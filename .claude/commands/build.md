Build the project using CMake + Qt6 + Ninja.

**Note**: This example uses macOS paths. Adjust for your platform:
- Windows: Use appropriate Qt path and ninja location
- Linux: Typically `$HOME/Qt/6/gcc_64/...` and system ninja

Steps:
1. Remove stale cache: `rm -rf cmake-build/CMakeCache.txt cmake-build/CMakeFiles`
2. Configure:
```bash
cmake -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_TOOLCHAIN_FILE=$HOME/Qt/6/macos/lib/cmake/Qt6/qt.toolchain.cmake \
  -DCMAKE_MODULE_PATH=$PWD/cmake \
  -G Ninja \
  -DCMAKE_C_COMPILER=clang \
  -DCMAKE_CXX_COMPILER=clang++ \
  -DCMAKE_MAKE_PROGRAM=/opt/homebrew/bin/ninja \
  -S . -B cmake-build
```
3. Build: `cmake --build cmake-build --parallel --verbose`
