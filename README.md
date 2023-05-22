# grpc-devdb

This service provides information related to device used in Fermilab's control system. The service uses gRPC for its protocol.

## Building

```shell
$ git clone git@github.com:fermi-controls/grpc-devdb.git
$ cd grpc-devdb
$ cargo build --release
```

The executable will be found at `target/release/devdb`.
