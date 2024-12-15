# **Gigs**

[![MIT/Apache 2.0](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/ecoskey/gigs#license)
[![crates.io](https://img.shields.io/crates/v/gigs)](https://crates.io/crates/gigs)
[![docs.rs](https://img.shields.io/docsrs/gigs?label=3D%20docs.rs)](https://docs.rs/gigs)

---

## On-demand graphics jobs for Bevy

Gigs is a plugin for the Bevy game engine that aims to provide a simple
abstraction for "graphics jobs", units of rendering work that only need to be
done sporadically, on-demand. For example, a terrain generation compute shader
would only need to be run once for each chunk of terrain. In many cases, this
crate will allow you to skip most or all of the manual extraction and resource
prep boilerplate that comes along with this, and focus on writing shaders.

### Warning

This library is still under development. `main` is very unstable, and may be broken
frequently. It's not feature-complete yet, and there will be churn as features like
job dependencies are added, and the code refactored.

## Getting Started

1. First, add `gigs` to your Cargo dependencies: `cargo add gigs`
2. Add `GraphicsJobsPlugin` to your `App`
3. Implement `GraphicsJob` for your job component
4. Call `init_graphics_job` on `App` to initialize your custom job
5. To run the job, simply spawn an entity with your job component!

## Supported Bevy Versions

| Bevy    | Gigs |
| ------- | ---- |
| 0.15    | 0.1  |

## License

Gigs may be licensed under either the MIT or Apache 2.0 licenses, at your option:

- MIT License ([LICENSE-MIT](/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
- Apache License, Version 2.0 ([LICENSE-APACHE](/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
