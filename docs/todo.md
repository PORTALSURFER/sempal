- turn the left and right sidebars into resizable panels.

- if we create a new sample in a collection by drag dropping an audio selection into the collection, and then restart the app, the collection item breaks.
I believe we are currently creating the item and place the file in some temp folder, but it should get created at the location of the collection export path and mapped to that

- our CI is tripper over missing dependencies. we will need to skip testing these areas I think as we are in the github actions environment here.

  --- stderr

  thread 'main' (5616) panicked at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/alsa-sys-0.3.1/build.rs:13:18:

  pkg-config exited with status code 1
  > PKG_CONFIG_ALLOW_SYSTEM_LIBS=1 PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1 pkg-config --libs --cflags alsa

  The system library `alsa` required by crate `alsa-sys` was not found.
  The file `alsa.pc` needs to be installed and the PKG_CONFIG_PATH environment variable must contain its parent directory.
  The PKG_CONFIG_PATH environment variable is not set.

  HINT: if you have installed the library, try setting PKG_CONFIG_PATH to the directory containing `alsa.pc`.

  note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
warning: build failed, waiting for other jobs to finish...
  --- stderr

  thread 'main' (5616) panicked at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/alsa-sys-0.3.1/build.rs:13:18:

  pkg-config exited with status code 1
  > PKG_CONFIG_ALLOW_SYSTEM_LIBS=1 PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1 pkg-config --libs --cflags alsa

  The system library `alsa` required by crate `alsa-sys` was not found.
  The file `alsa.pc` needs to be installed and the PKG_CONFIG_PATH environment variable must contain its parent directory.
  The PKG_CONFIG_PATH environment variable is not set.

  HINT: if you have installed the library, try setting PKG_CONFIG_PATH to the directory containing `alsa.pc`.

  note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
warning: build failed, waiting for other jobs to finish...

- if focus goes away from the sample browser context, for example, I focus an item in the collection list. make sure to stop focussing sample items in the browser 
