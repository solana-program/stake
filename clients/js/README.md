# JavaScript client

A generated JavaScript library for the Stake program.

## Getting started

The JS client tests use [LiteSVM](https://github.com/LiteSVM/litesvm) in-process, so no local validator is needed. To build and test your JavaScript client from the root of the repository, you may use the following command.

```sh
make test-js-clients-js
```

This installs dependencies, builds the client, and runs the test suite.

## Available client scripts.

Alternatively, you can go into the client directory and run the tests directly.

```sh
# Build the program `.so` that LiteSVM loads.
make build-sbf-program

# Go into the client directory and run the tests.
cd clients/js
pnpm install
pnpm build
pnpm test
```

You may also use the following `make` targets from the root of the repository to lint and format your JavaScript client:

```sh
make lint-js-clients-js
make format-check-js-clients-js
```
