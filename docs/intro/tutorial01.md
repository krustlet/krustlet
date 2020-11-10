# Writing your first app, part 1

Let’s learn by example.

Throughout this tutorial, we’ll walk you through the creation of a basic WASI
application. Once ready, we will package that application and install it onto
the Kubernetes cluster using krustlet.

The tutorial will consist of three parts:

- Building the application
- Publishing the application to a registry
- Running the application with Krustlet

## Prerequisites

We’ll assume you have Cargo (a package management system for Rust) installed
already.

If you're compiling the application written in C, you'll want to install the
[WASI SDK](https://github.com/WebAssembly/wasi-sdk), though if you're following
the tutorial with the Rust example, this step is optional.

In part 2 of this tutorial, we will be publishing our application to a registry
hosted on Microsoft Azure. The steps assume you have an Azure account and the
`az` CLI installed. However, there are other cloud providers available with
their own solutions, and if you're feeling particularly brave, you can [run your
own registry on your own
infrastructure](https://github.com/docker/distribution). You'll also need
[wasm-to-oci](https://github.com/engineerd/wasm-to-oci) (a tool for publishing
WebAssembly modules to a registry).

We’ll assume you have Krustlet installed already. See [the quickstart
guide](quickstart.md) for advice on how to boot a Kubernetes cluster and install
Krustlet.

If you're having trouble going through this tutorial, please post an issue to
[deislabs/krustlet](https://github.com/deislabs/krustlet) to chat with other
Krustlet users who might be able to help.

## Creating your first application

For this tutorial, we'll be creating an example application written either in C
or in Rust.

The application a very simple "hello world" application, running forever and
printing "hello world!" every 5 seconds to standard output.

### Option 1: From C

First, let's write the application in C. To create your app, type this command:

```console
$ mkdir demo
$ cd demo
$ touch main.c
```

The C code here uses standard POSIX APIs, and doesn't have any knowledge of WASI
internals.

```c
#include <stdio.h>
#include <unistd.h>

int main() {
    while(1) {
        printf("Hello, World!\n");
        sleep(5);
    }
    return 0;
}
```

The wasi-sdk provides a clang which is configured to target WASI. We can compile
our program like so:

```console
$ clang main.c -o demo.wasm
```

This is just regular clang, configured to use a WebAssembly target and sysroot.
The output of clang here is a standard WebAssembly module:

```console
$ file demo.wasm
demo.wasm: WebAssembly (wasm) binary module version 0x1 (MVP)
```

### Option 2: From Rust

The same application can be written in Rust. First, go ahead and start a new
project:

```console
$ cargo new --bin demo
```

Now, let's port the C program defined earlier to Rust. In `src/main.rs`:

```rust
use std::time::Duration;
use std::thread::sleep;

fn main() {
    loop {
        println!("Hello, World!");
        sleep(Duration::from_secs(5));
    }
}
```

In order to build it, we first need to install a WASI-enabled Rust toolchain:

```console
$ rustup target add wasm32-wasi
$ cargo build --release --target wasm32-wasi
```

We should now have the WebAssembly module created in `target/wasm32-wasi/release`:

```console
$ file target/wasm32-wasi/release/demo.wasm
demo.wasm: WebAssembly (wasm) binary module version 0x1 (MVP)
```

## Optional: executing with wasmtime

The WebAssembly module `demo.wasm` we just compiled either from C or Rust is
simply a single file containing a self-contained WASM module.

`wasmtime` is a standalone JIT-style runtime for WebAssembly and WASI. It runs
WebAssembly code outside of the web, and can be used both as a command-line
utility or as a library embedded in a larger application.

We can execute our application with `wasmtime` directly, like so:

```console
$ wasmtime demo.wasm
Hello, World!
Hello, World!
Hello, World!
^C
```

To exit the program, enter CTRL+C with your keyboard.

Great! Our program runs as expected!

When you’re comfortable with the application, read [part 2](tutorial02.md) of
this tutorial to learn about publishing our application to a registry, where
Krustlet will be able to find it and run it.
