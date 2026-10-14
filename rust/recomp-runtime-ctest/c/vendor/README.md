`lightweightsemaphore.h` is from Cameron Desrochers' [concurrentqueue](https://github.com/cameron314/concurrentqueue)
(simplified BSD or Boost license; Jeff Preshing's semaphore code inside it is zlib-licensed),
copied unchanged from N64ModernRuntime's `thirdparty/concurrentqueue`. It's here only so the
C++ save backend in `../saving_reference.cpp` builds with the same semaphore it uses in the runtime.
