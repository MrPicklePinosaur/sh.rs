---
title: "Build a unix shell in Rust using shrs"
url: "/blog/build-a-shell-in-rust/"
description: "Learn how you can build your own unix shell in minutes using shrs - the rusty shell toolkit"
summary: ""
date: 2024-06-01
lastmod: 2024-06-01
draft: false
weight: 50
categories: []
tags: []
pinned: false
homepage: false
extra:
  authors:
    - name: "pinosaur"
      href: "https://github.com/MrPicklePinosaur"
---

## 0. What even is a shell?

At it's most basic, a [shell](https://en.wikipedia.org/wiki/Unix_shell) is a program that executes other programs. It provides an interface using text commands from the user written in the shell's interpreted language, and may support features like scripting, intelligent tab completion, command history, and more. In the Linux world, there are a number of options for shells you can use, the most popular ones being [bash](https://en.wikipedia.org/wiki/Bash_(Unix_shell)) (installed by default on most Linux distributions) and [zsh](https://en.wikipedia.org/wiki/Z_shell) (installed by default on MacOS). However, these shells have existed for more than thirty years, carrying much historical baggage. We have seen the emergence of modern shells, with novel ideas and new paradigms, notably [nushell](https://www.nushell.sh/) - with structured data and written in Rust, as well as [fish](https://fishshell.com/) - prioritizing user friendliness and delightful completions.

## So what's shrs?

[shrs](https://github.com/MrPicklePinosaur/shrs) isn't just another shell that's written in Rust, it's a toolkit for you to create your own shell! It can also be thought of as a shell whose _configuration language_ is Rust. **shrs** gives you a framework to build a shell exactly how you like it, providing an order of magnitude more control than existing shell configuration.

## 1. Getting started

The pseudocode below provides a highly simplified view on how a shell functions.
```
loop {
    input = get_user_input()
    command = parse_command(input)
    run_command(command)
}
```
In short, it runs an infinite loop that 
1) _Reads a line of input from the user_ - while reading this line, the shell can provide features to make the user's life easier, things like tab completion, vi mode, history, syntax highlighting, and much more. Once the user presses `Enter`, the command is accepted
2) _Parsed by the shell interpreter_ - shells provide a 'programming language' that is able to execute complex user commands, which means shells need to implement an interpreter akin to other interpreted languages - like python
3) _Command execution_ - after the shell has converted the text input into a format it can understand, it asks the operating system to do what the user requested; this may be spawning a program, opening a file or setting up pipes for processes to talk to each other

<!-- Traditionally, this job is done by the [GNU readline](https://en.wikipedia.org/wiki/GNU_Readline) program, but **shrs** provides it's own configurable version implemented in Rust. -->

// TODO provide github repo with the code for this blog post

**shrs** provides a framework that handles the heavy lifting of the core shell logic and lets you focus on the components that matter. In fact, you can produce a working shell in just a couple lines of code! Let's get started by creating a new Rust project. If you don't have Rust set up already, [install](https://www.rust-lang.org/tools/install) it using rustup (recommended). With Rust installed, let's create a new project using cargo
```sh
cargo new --bin <project-name>
cd <project-name>
```
First we need to add **shrs** as a dependency in your `Cargo.toml` file
```toml
[dependencies]
shrs = { version = "0.0.6" }
```
Then in `main.rs`, we can create the most basic **shrs** shell
```rust
use shrs::prelude::*;

fn main() {
    let myshell = ShellBuilder::default()
        .build()
        .unwrap();

    myshell.run();
}
```
Breaking down each line:
- `use shrs::prelude::*` - imports most of the functions, traits and modules that you will need
- `ShellBuilder::default()` - creates a new shell configuration, this is the one-stop shop for all config options. See the [docs](https://docs.rs/shrs/latest/shrs/prelude/struct.ShellBuilder.html) for more info. Once we are happy with the options, we call `build()` to finalize the configuration and produce a runnable shell.
- `myshell.run()` - start running the shell loop

And that's it! We have a working shell in just a couple of lines of code! Let's run the shell using
```sh
cargo run
```

// TODO insert gif of basic shell

The default shell with no configuration options is quite bare bones however. Let's start by spicing up our prompt

## 2. Prompt
If you want to customize your prompt in a shell like bash, this is what it would look like (credit [reddit](https://www.reddit.com/r/linux/comments/2uf5uu/this_is_my_bash_prompt_which_is_your_favorite/))
```sh
PS1="\[\033[m\]|\[\033[1;35m\]\t\[\033[m\]|\[\e[1;31m\]\u\[\e[1;36m\]\[\033[m\]@\[\e[1;36m\]\h\[\033[m\]:\[\e[0m\]\[\e[1;32m\][\W]> \[\e[0m\]"
```
...what?? Why is it so cryptic?

Fear not, in **shrs** it's a _lot_ more readable and ergonomic. We begin by defining a regular ol' function and format the prompt using a handy macro called [styled_buf](https://docs.rs/shrs/latest/shrs/prelude/macro.styled_buf.html) that let's us easily put together a prompt with styles ([crossterm::Stylize](https://docs.rs/crossterm/latest/crossterm/style/trait.Stylize.html) is used under the hood). Here is the same prompt recreated in **shrs**.
```rust
fn prompt_left() -> StyledBuf {
    styled_buf!(
        "|",
        "|",
        username().map(|u| u.red().bold()),
        "@",
        hostname().map(|u| u.blue().bold()),
        "[".green().bold(),
        top_pwd().green().bold(),
        "]>".green().bold()
    )
}

// Placeholder right prompt
fn prompt_right() -> StyledBuf {
    styled_buf!()
}
```
Note that we have an empty `prompt_right` for now. Once we have our function written, we just need to register the prompt when constructing the shell.
```rust
use shrs::prelude::*;

fn main() {
    let myshell = ShellBuilder::default()
        .with_prompt(Prompt::from_sides(prompt_left, prompt_right))
        .build()
        .unwrap();

    myshell.run();
}
```

And here's what it looks like

// TODO insert gif

Let's go above and beyond with our prompt. Another useful feature is being able to display the current git branch so let's implement that next!

## 3. Plugins

**shrs** has a rich plugin ecosystem of both official and third party plugins - which add functionality in a modular fashion. In addition, plugins are just crates, so you can share and make use of plugins just like any other library! A list of plugins can be found on [crates.io](https://crates.io/search?q=shrs) or on the [awesome list](https://github.com/MrPicklePinosaur/awesome_shrs). Examples of cool plugins include
- [shrs_insulter](https://github.com/nithinmuthukumar/shrs_insulter): insults you when you mistype a command
- [shrs_presence](https://github.com/nithinmuthukumar/shrs_presence): show off what you are doing in the shell on Discord 
- [shrs_openai](https://github.com/MrPicklePinosaur/shrs_openai): AI for the command line! Ask questions or generate commands
- [shrs_sound](https://github.com/nithinmuthukumar/shrs_sound): play sound effects in the shell

To add git information to our prompt, we will make use of a plugin called [shrs_cd_tools](https://github.com/MrPicklePinosaur/shrs/tree/master/plugins/shrs_cd_tools), which is able to query the filesystem and extract information like git information, as well as what type of project (rust, node, python etc). To get started, let's add it to our `Cargo.toml`.
```toml
[dependencies]
shrs = { version = "0.0.6" }
shrs_cd_tools = { version = "0.0.6" }
```
Next, we register the plugin with the shell
```rust
use shrs::prelude::*;
use shrs_cd_tools::{DirParsePlugin, DirParseState, git::Git};

fn main() {
    let myshell = ShellBuilder::default()
        .with_plugin(DirParsePlugin::new())
        .with_prompt(Prompt::from_sides(prompt_left, prompt_right))
        .build()
        .unwrap();

    myshell.run();
}
```
The `cd_tools` plugin will parse the current directory for us and put the results of the parse in shell [State](https://mrpicklepinosaur.github.io/shrs/docs/shell-config/states/), which we can then query from our prompt. Let's add the git status to our `right_prompt`:
```rust

```

Awesome! Next, let's add a [fetch](https://github.com/beucismis/awesome-fetch) script that runs every time we start the shell.

## Hooks

To run arbitrary commands when the shell starts up, **shrs** exposes _hooks_
that you can register custom handlers with. This lets you respond to numerous
events that are emitted by the shell as well as third party plugins, some of
which include:
- shell startup
- change working directory
- finish running a command
- pressing a key





