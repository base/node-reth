# `base-builder-multiplex`

Runs Base Flashblocks and native (basic) payload builders behind a single routing
`PayloadBuilderHandle`. With cutover mode enabled, Flashblocks is selected through Beryl;
any active upgrade after Beryl selects the native builder.

## Overview

- with cutover mode enabled, fans out every `BuildNewPayload` request to both builders,
- before the first post-Beryl upgrade, selects Flashblocks and runs the native builder as a
  `no_tx_pool` shadow,
- at and after activation, selects native and runs Flashblocks as a `no_tx_pool` shadow,
  which does not publish Flashblocks,
- routes reads (`BestPayload`, `PayloadTimestamp`, `Resolve`, `Subscribe`) to the builder
  selected for each payload,
- with basic-only mode enabled, starts only the native payload builder, without Flashblocks,
- defaults cutover mode to disabled, preserving plain `FlashblocksServiceBuilder` startup.

## Startup configuration

Enable `--builder.payload-builder-cutover` before activation. It can remain enabled across
restarts after activation. To stop running Flashblocks entirely, use
`--builder.basic-payload-builder` instead. The flags are mutually exclusive.

In cutover mode, routing checks the effective fork schedule for each payload timestamp, including
runtime upgrade signals. All upgrades ordered after Beryl are considered, even if Cobalt is unscheduled.
Already-created payloads retain their recorded route. This changes builder eligibility, not the
consensus block-time schedule.
