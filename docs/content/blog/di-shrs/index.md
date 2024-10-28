---
title: "Implementing Dependency Injection in rust"
url: "/blog/doks-1/"
description: "Improving library ergonomics using dependency injection"
summary: "the development process behind implementing dependency injection in rust, and its advantages"
date: 2024-05-28
lastmod: 2024-05-28
draft: false
weight: 50
categories: []
tags: []
pinned: false
homepage: false
extra:
  authors:
    - name: "Nithin"
      href: "https://github.com/nithinmuthukumar"

---
[Dependency Injection](https://en.wikipedia.org/wiki/Dependency_injection#:~:text=Dependency%20injection%20aims%20to%20separate,how%20to%20construct%20those%20services.) is a design pattern that involves providing dependencies to a component from an external source rather than creating them internally. It's used in libraries such as axum and bevy to provide a clean interface.


This blog post explores my journey of implementing dependency injection (DI) in Rust, specifically focusing on its application within the shrs library. shrs aims to provide an API for accessing various shell states through handlers such as keybindings and hooks, at different points in the shell execution process. Doing this requires a shared mutable state, accessible in all the various handlers.

Traditionally, a direct solution to managing shared states involves using global mutable state. However, this approach introduces several issues, particularly when designing a library. As a consequence, the shrs library had to rely on passing mutable references to states throughout the codebase, resulting in verbose traits in the user-facing API.
```rust
// example of a verbose handler trait
// sh, ctx and rt represent global state, themselves containing various values
pub trait BuiltinCmd {
    fn run(
        &self,
        sh: &Shell,
        ctx: &mut Context,
        rt: &mut Runtime,
        args: &Vec<String>,
    ) -> anyhow::Result<CmdOutput>;
}
```
Moreover, user-defined states were managed using an `AnyMap`, further complicating access and exacerbating verbosity in the codebase. These challenges persisted since the inception of shrs, prompting me to explore solutions to enhance library ergonomics and maintainability.

### Implementation
Having prior experience with Bevy, I found the idea of implementing a similar system in SHRS a great idea. The concept of having handlers that resemble systems, with variable parameters and the ability to access requested resources, which is dependency injection.


After searching around I found this tutorial, [Dependency Injection like Bevy Engine from Scratch](https://promethia-27.github.io/dependency_injection_like_bevy_from_scratch/introductions.html). I was able to quickly adapt the provided code for the various handlers. The idea is that handlers only need to implement a trait, which can be implemented on Fn with some number of parameters.
```rust

```
The trick is to use a macro to implement it for as many parameters as is wanted.

It instantly felt like a great improvement. For example, writing a builtin went from [this](https://github.com/MrPicklePinosaur/shrs/blob/e2f839806d37108120394e3e5cdfad495ce2701c/crates/shrs_core/src/builtin/help.rs) to [this](https://github.com/MrPicklePinosaur/shrs/blob/master/crates/shrs_core/src/builtin/help.rs).

Users no longer needed to create a struct for a single method and the user only needed to have params for what they needed.

State was still broken into two pieces, since Shell had to be immutable. Shell stores the systems that are run and its impossible to pass a mutable reference of Shell to the systems. This led to many limitations where it was impossible to modify anything in Shell. Bevy faces a similar problem with inserting resources at runtime, and I copied their [method](https://w.com). Basically, inside of a system users are able to queue up changes that will occur directly after the callback ends. This means that the mutable reference will be dropped and Shell is modifiable.

## Optional Parameters

## Lingering problems
