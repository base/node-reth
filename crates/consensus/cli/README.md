# `base-consensus-cli`

CLI argument types for Base consensus clients.

## Overview

This crate provides reusable CLI argument types for configuring Base consensus clients:

- **`L1ClientArgs`**: L1 execution client RPC configuration
- **`L2ClientArgs`**: L2 engine API configuration with JWT handling
- **`RpcArgs`**: JSON-RPC server configuration
- **`SequencerArgs`**: Sequencer mode configuration

## Usage

Standalone and integrated node startup both use
`ConsensusNodeArgs::start_with_options(ConsensusNodeStartOptions)`.
The options carry the rollup configuration, endpoint overrides, cancellation token,
and whether upgrade-signal startup has already been applied by the embedded execution node.
There is no separate startup path for each combination of these options.

```toml
[dependencies]
base-consensus-cli = { workspace = true }
```

```rust
use base_consensus_cli::{L1ClientArgs, L2ClientArgs};
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[clap(flatten)]
    l1_args: L1ClientArgs,
    #[clap(flatten)]
    l2_args: L2ClientArgs,
}
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
