[![License Apache 2](https://img.shields.io/badge/License-Apache2-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Crates.io](https://img.shields.io/crates/v/stan.svg)](https://crates.io/crates/stan)
[![Documentation](https://docs.rs/stan/badge.svg)](https://docs.rs/stan/)
[![Build Status](https://travis-ci.com/ReifyAB/stan-rs.svg?branch=main)](https://travis-ci.com/ReifyAB/stan-rs)

# {{crate}}

{{readme}}

## Installation

```toml
[dependencies]
nats = "0.9.7"
stan = "{{version}}"
```

## Development

To start a local nats streaming server:

```sh
docker run -p 4222:4222 -p 8222:8222 nats-streaming
```

Running tests require docker. To run the tests:

```sh
cargo test
```
