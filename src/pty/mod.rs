//! PTY management layer

use anyhow::Result;
use bytes::BytesMut;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 40;

pub struct PtyHandle {
    write_tx: mpsc::Sender<PtyCommand>,
    output_tx: broadcast::Sender<Vec<u8>>,
    buffer: Arc<Mutex<BytesMut>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    shutdown: Arc<AtomicBool>,
}

enum PtyCommand {
    Write {
        data: Vec<u8>,
        response: oneshot::Sender<Result<()>>,
    },
    Resize {
        cols: u16,
        rows: u16,
        response: oneshot::Sender<Result<()>>,
    },
    Shutdown,
}

impl PtyHandle {
    pub fn spawn_command(
        command: &[String],
        working_dir: &Path,
        buffer_limit: usize,
    ) -> Result<Self> {
        if command.is_empty() {
            anyhow::bail!("Empty command");
        }

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(&command[0]);
        for arg in &command[1..] {
            cmd.arg(arg);
        }

        if working_dir.exists() {
            cmd.cwd(working_dir);
        }

        // Spawn command and save child handle
        let child = pair.slave.spawn_command(cmd)?;
        let child: Arc<Mutex<Box<dyn Child + Send + Sync>>> = Arc::new(Mutex::new(child));

        // Get reader BEFORE moving master
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;

        let (output_tx, _) = broadcast::channel(1024);
        let buffer = Arc::new(Mutex::new(BytesMut::with_capacity(buffer_limit)));

        // Channel for write commands
        let (write_tx, mut write_rx) = mpsc::channel::<PtyCommand>(256);

        // Master handle for resize - move after getting reader
        let master = pair.master;

        // Shutdown flag
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_writer = shutdown.clone();
        let shutdown_reader = shutdown.clone();

        // Spawn write handler thread
        std::thread::spawn(move || {
            while let Some(cmd) = write_rx.blocking_recv() {
                match cmd {
                    PtyCommand::Write { data, response } => {
                        let result = writer
                            .write_all(&data)
                            .and_then(|_| writer.flush())
                            .map_err(|e| anyhow::anyhow!("{}", e));
                        let _ = response.send(result);
                    }
                    PtyCommand::Resize {
                        cols,
                        rows,
                        response,
                    } => {
                        let result = master
                            .resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            })
                            .map_err(|e| anyhow::anyhow!("{}", e));
                        let _ = response.send(result);
                    }
                    PtyCommand::Shutdown => {
                        shutdown_writer.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });

        // Spawn output reader thread
        let output_tx_clone = output_tx.clone();
        let buffer_clone = buffer.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if shutdown_reader.load(Ordering::SeqCst) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let data = buf[..n].to_vec();

                        // Broadcast to WebSocket subscribers
                        let _ = output_tx_clone.send(data.clone());

                        // Store in ring buffer synchronously using blocking lock
                        // Skip buffer updates if buffer_limit is 0
                        if buffer_limit > 0 {
                            let mut buf_lock = buffer_clone.blocking_lock();

                            // If data chunk is larger than buffer limit, keep only the last buffer_limit bytes
                            if data.len() >= buffer_limit {
                                buf_lock.clear();
                                let start = data.len() - buffer_limit;
                                buf_lock.extend_from_slice(&data[start..]);
                            } else {
                                // Normal case: make room for new data
                                let new_total = buf_lock.len() + data.len();
                                if new_total > buffer_limit {
                                    let to_remove = new_total - buffer_limit;
                                    let _ = buf_lock.split_to(to_remove);
                                }
                                buf_lock.extend_from_slice(&data);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            write_tx,
            output_tx,
            buffer,
            child,
            shutdown,
        })
    }

    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.write_tx
            .send(PtyCommand::Write {
                data: data.to_vec(),
                response: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("PTY channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Response channel closed"))?
    }

    pub async fn write_line(&self, line: &str) -> Result<()> {
        self.write(format!("{}\n", line).as_bytes()).await
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    pub async fn get_buffer(&self) -> Vec<u8> {
        self.buffer.lock().await.to_vec()
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.write_tx
            .send(PtyCommand::Resize {
                cols,
                rows,
                response: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("PTY channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Response channel closed"))?
    }

    /// Kill the child process
    pub async fn kill(&self) -> Result<()> {
        let mut child = self.child.lock().await;
        child.kill().map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Wait for child process to exit
    pub async fn wait(&self) -> Result<portable_pty::ExitStatus> {
        let mut child = self.child.lock().await;
        child.wait().map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Check if child process is still running
    pub async fn try_wait(&self) -> Result<Option<portable_pty::ExitStatus>> {
        let mut child = self.child.lock().await;
        child.try_wait().map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Graceful shutdown: send signal, wait briefly, then force kill
    pub async fn shutdown(&self) {
        // Signal shutdown to worker threads
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.write_tx.send(PtyCommand::Shutdown).await;

        // Try to kill the child process
        if let Err(e) = self.kill().await {
            tracing::warn!("Failed to kill child process: {}", e);
        }

        // Wait briefly for process to exit
        let wait_result = tokio::time::timeout(std::time::Duration::from_millis(500), async {
            loop {
                match self.try_wait().await {
                    Ok(Some(_)) => return true,
                    Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
                    Err(_) => return false,
                }
            }
        })
        .await;

        if wait_result.is_err() || wait_result == Ok(false) {
            tracing::warn!("Child process did not exit gracefully");
        }
    }
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        // Set shutdown flag
        self.shutdown.store(true, Ordering::SeqCst);

        // Try to kill child process synchronously
        // Note: We can't use async here, so we use try_lock
        if let Ok(mut child) = self.child.try_lock() {
            if let Err(e) = child.kill() {
                tracing::debug!("Failed to kill child on drop: {}", e);
            }
        }
    }
}

pub struct PtyManager {
    handles: Arc<Mutex<std::collections::HashMap<String, Arc<PtyHandle>>>>,
    buffer_limit: usize,
}

impl PtyManager {
    pub fn new(buffer_limit: usize) -> Self {
        Self {
            handles: Arc::new(Mutex::new(std::collections::HashMap::new())),
            buffer_limit,
        }
    }

    pub async fn create(
        &self,
        agent_name: &str,
        command: &[String],
        working_dir: &Path,
    ) -> Result<Arc<PtyHandle>> {
        let handle = Arc::new(PtyHandle::spawn_command(
            command,
            working_dir,
            self.buffer_limit,
        )?);
        self.handles
            .lock()
            .await
            .insert(agent_name.to_string(), handle.clone());
        Ok(handle)
    }

    pub async fn get(&self, agent_name: &str) -> Option<Arc<PtyHandle>> {
        self.handles.lock().await.get(agent_name).cloned()
    }

    pub async fn remove(&self, agent_name: &str) -> Option<Arc<PtyHandle>> {
        self.handles.lock().await.remove(agent_name)
    }

    pub async fn list(&self) -> Vec<String> {
        self.handles.lock().await.keys().cloned().collect()
    }

    pub fn buffer_limit(&self) -> usize {
        self.buffer_limit
    }

    /// Shutdown all PTY handles
    pub async fn shutdown_all(&self) {
        let handles: Vec<_> = self.handles.lock().await.drain().collect();
        for (name, handle) in handles {
            tracing::info!("Shutting down PTY for agent: {}", name);
            handle.shutdown().await;
        }
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        // Try to kill all child processes synchronously
        if let Ok(handles) = self.handles.try_lock() {
            for (name, handle) in handles.iter() {
                if let Ok(mut child) = handle.child.try_lock() {
                    if let Err(e) = child.kill() {
                        tracing::debug!("Failed to kill {} on manager drop: {}", name, e);
                    }
                }
            }
        }
    }
}
