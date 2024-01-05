---
title: Learning Sui
---

*Sui: pronounced "sweet" without the "T" - with Transactions (loads of them), things are SWEET indeed. :-)*

Welcome to the documentation for the Sui platform. Since Sui is built upon the core [Move](https://github.com/MystenLabs/awesome-move)
programming language, you should familiarize yourself with it and use this content to apply the differences. For a summary of these differences, see
[Sui compared to other blockchains](../learn/sui-compared.md).

For a deep dive into Sui technology, see the [Sui Smart Contracts Platform](https://github.com/MystenLabs/sui/blob/main/doc/paper/sui.pdf) white paper. Find answers to common questions about our [roadmap](https://github.com/MystenLabs/sui/blob/main/ROADMAP.md) and more in our [FAQ](../contribute/faq.md).

> **Important:** This site is built from the upstream `main` branch and therefore will contain updates not yet found in `devnet`.

## See what's new

Find the latest updates to these contents in this section:

* Sui now supports development using [Microsoft Windows 11, macOS, and Linux](../build/install.md#supported-oses). See [install Sui](../build/install.md#prerequisites) for the prerequisites of each operating system.
* `sui::id` is now `sui::object` and `VersionedID` is now `Info`. Use the [Object module](https://github.com/MystenLabs/sui/blob/main/crates/sui-framework/sources/object.move) that has [replaced the former ID.move.](https://github.com/MystenLabs/sui/pull/3241)
* Find a list of [single-writer apps](../learn/single-writer-apps.md) that would benefit from Sui's advantages in handling [simple transactions](../learn/how-sui-works.md#simple-transactions).
* Sui [version 0.6.1](https://github.com/MystenLabs/sui/releases/tag/devnet-0.6.1) released to DevNet. See the [#sui-release-notes](https://discord.com/channels/916379725201563759/974444055259910174) channel in Discord for details on this and prior releases.
* Install the [Sui Wallet Browser Extension](../explore/wallet-browser.md) to create NFTs, transfer coins, and carry out common transactions in a Chrome tab.
* [Sui Move is feature complete](https://sui.io/resources-move/why-we-created-sui-move/) and ready for you to write safe and efficient smart contracts. See https://examples.sui.io/ to learn Sui Move by example.
* If your application is written in JavaScript or TypeScript, follow the [TypeScript SDK documentation](https://github.com/MystenLabs/sui/tree/main/sdk/typescript) and [reference files](https://www.npmjs.com/package/@mysten/sui.js).

For a complete view of all changes in the Sui `devnet` branch, see:
https://github.com/MystenLabs/sui/commits/devnet

For upstream updates in the `main` branch, see:
https://github.com/MystenLabs/sui/commits/main

See the Sui `doc/src` [history](https://github.com/MystenLabs/sui/commits/main/doc/src) for a complete changelog of updates to this site. 

## Kickstart development

### Write Smart Contracts with Move
Go to the [Move Quick Start](../build/move/index.md) for installation, defining custom objects, object operations (create/destroy/update/transfer/freeze), publishing, and invoking your published code.

### Start the Sui network with Sui CLI client
See the [Sui CLI client Quick Start](../build/cli-client.md) for installation, querying the chain, client setup, sending transfer transactions, and viewing the effects.

### Take the tic-tac-toe tutorial
Proceed to the [Sui Tutorial](../explore/tutorials.md) for a summary view of setting up your environment, starting a Sui network, gathering accounts and gas, and publishing and playing a game in Sui.

### Program with Objects
Finish with the detailed [Programming with objects](../build/programming-with-objects/index.md) tutorial series offering detailed guidance on manipulating Sui objects, from creation and storage through wrapping and using child objects.

## Navigate this site

Navigate and search this site however you see fit. Here is the order we recommend if you are new to Sui:

1. Learn [about Sui](../learn/about-sui.md), how [Sui Move differs from Core Move](../learn/sui-move-diffs.md), and [how Sui works](../learn/how-sui-works.md) starting in this very section.
1. [Build](../build/index.md) smart contracts, the Sui client, a Sui fullnode, and more.
1. [Explore](../explore/index.md) prototypes and examples.
1. [Contribute](../contribute/index.md) to Sui by joining the community, making enhancements, and learning about Mysten Labs.
1. Employ the [Sui API Reference](https://playground.open-rpc.org/?uiSchema%5BappBar%5D%5Bui:splitView%5D=false&schemaUrl=https://raw.githubusercontent.com/MystenLabs/sui/main/crates/sui-open-rpc/spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:input%5D=false) reference files for the [Sui JSON-RPC API](../build/json-rpc.md).
1. View the [Mysten Labs](https://www.youtube.com/channel/UCI7pCUVxSLcndVhPpZOwZgg) YouTube channel for introductory videos on technology and partners.
