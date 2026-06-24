# building-tcp-servers-in-rust

One TCP echo server, built seven times, from a blocking socket up to an async
runtime. Each version is a self-contained module that spells out its own I/O
loop. They all pass the same test.

This is the Rust companion to
[building-tcp-servers-in-c](https://github.com/valentynkit/building-tcp-servers-in-c).
The C project tells the story of the I/O models. This one tells a story C
cannot: how Rust's abstraction tower is built, from raw `unsafe` libc syscalls,
through a safe cross-platform reactor, up to `async`/`await`.

## The seven stages

Each stage hits a concrete wall. The next stage is the answer to it.

| # | Stage | Mechanism | Wall it answers |
|---|---|---|---|
| 1 | blocking | thread per connection | one slow client blocks everyone |
| 2 | nonblocking | `set_nonblocking`, busy poll | thread per connection stops scaling, but spinning burns a core |
| 3 | select | `libc::select` | readiness instead of spin, but O(n) and the `FD_SETSIZE` cap |
| 4 | poll | `libc::poll` | the same idea without the fd-count cap, still O(n) |
| 5 | eventloop | epoll or kqueue | O(ready) dispatch, register an fd once |
| 6 | mio | safe cross-platform reactor | stop hand-writing per-kernel `unsafe` |
| 7 | tokio | async runtime | stop hand-writing the reactor, write `async fn` |

Stages 1 and 2 are safe `std`. Stages 3 to 5 call the kernel directly, which is
where the `unsafe` and the FFI appear, because `select`, `poll`, `epoll`, and
`kqueue` are not in the standard library. Stage 5 is one stage compiled two
ways: kqueue on macOS and the BSDs, epoll on Linux.

## The abstraction tower

```
stage 1-2   std            blocking, then nonblocking, no kernel readiness API
stage 3-5   unsafe libc    select, poll, epoll/kqueue, by hand
stage 6     mio            a safe reactor, you still drive the loop
stage 7     tokio          async/await, the loop drives itself
```

mio wraps exactly the syscalls stage 5 writes out. tokio is a reactor like the
one stage 6 builds, with `async`/`await` on top. Reading the stages in order is
the point: by stage 6 you know what mio is hiding, and by stage 7 you know what
the runtime stands on.

## Run it

```sh
cargo run -- --backend eventloop --port 9999
```

Every backend speaks the same raw echo, so any TCP client drives it:

```sh
$ nc localhost 9999
hello
hello
```

Swap `--backend` to compare the models without changing anything else. The
choices are `blocking`, `nonblocking`, `select`, `poll`, `eventloop`, `mio`,
`tokio`.

## The test

```sh
cargo test
```

One harness runs the same four checks against all seven backends: a single
echo, sixty-four concurrent clients, a one megabyte payload that spans many
reads, and a client that disconnects mid-stream without taking the server down.
The same test passing against every implementation is the proof they are
behaviorally identical. CI runs it on Linux and macOS, so epoll and kqueue are
both covered.

## Platforms

Unix only. Linux uses epoll, macOS and the BSDs use kqueue. There is no Windows
backend, since the whole point is the readiness syscalls those kernels expose.

## License

MIT or Apache-2.0, at your option.
