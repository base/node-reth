# `base-node`

<a href="https://github.com/base/base/actions/workflows/ci.yml"><img src="https://github.com/base/base/actions/workflows/ci.yml/badge.svg?label=ci" alt="CI"></a>
<a href="https://github.com/base/base/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-d1d1f6.svg?label=license&labelColor=2a2f35" alt="MIT License"></a>

Shared scaffolding for Base node and builder binaries. Provides extension traits, type aliases, and the `BaseNodeRunner` for building modular node extensions.

## Overview

- **`BaseNode`**: Type configuration for a regular Base node.
- **`BaseAddOns`**: Add-ons for Base node with RPC, engine API, and validation.
- **`BaseNodeRunner<P>`**: Generic runner that can be configured with different payload builders.
- **`BaseBuilder`**: Wrapper around the OP builder that accumulates hooks.
- **`BaseNodeExtension`**: Trait for node builder extensions that can apply additional wiring.
- **`FromExtensionConfig`**: Trait for extensions that can be constructed from a configuration type.
- **`OpBuilder`**: Type alias for the OP node builder with launch context.
- **`OpProvider`**: Type alias for the blockchain provider instance.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-node = { git = "https://github.com/base/base" }
```

### Using the default payload builder (for base-node)

```rust,ignore
use base_node::BaseNodeRunner;
use reth_optimism_node::args::RollupArgs;

let mut runner = BaseNodeRunner::with_default_payload(rollup_args);
runner.install_ext::<MyExtension>(config);
runner.run(builder).await;
```

### Using a custom payload builder (for base-builder)

```rust,ignore
use base_node::{BaseNodeRunner, BaseNodeConfig};
use my_crate::CustomPayloadBuilder;

let config = BaseNodeConfig::new(rollup_args, CustomPayloadBuilder::new());
let runner = BaseNodeRunner::new(config);
runner.run(builder).await;
```

### Implementing a custom extension

```rust,ignore
use base_node::{BaseNodeExtension, FromExtensionConfig, BaseBuilder};
use eyre::Result;

#[derive(Debug)]
struct MyExtension {
    // extension state
}

impl BaseNodeExtension for MyExtension {
    fn apply(self: Box<Self>, builder: BaseBuilder) -> BaseBuilder {
        // Apply custom wiring to the builder
        builder
    }
}

impl FromExtensionConfig for MyExtension {
    type Config = MyConfig;

    fn from_config(config: Self::Config) -> Self {
        Self { /* ... */ }
    }
}
```
