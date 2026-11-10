# NEON for OpenIDEA

Support for `.neon` files, the config format of [Nette](https://nette.org) and
[PHPStan](https://phpstan.org): `common.neon`, `phpstan.neon`, `phpstan.neon.dist`,
`phpstan-baseline.neon`, …

Install it and open a `.neon` file. There's nothing to set up.

## Features

- **Highlighting**, **structure view** and **folding**.
- **Errors while typing**: the ones nette/neon reports (syntax, indentation, tabs mixed
  with spaces, unclosed brackets, duplicate keys), all of them at once.
- **nette/neon 3 warnings**: where PHPStan or Nette 3 would read the file differently.
- **Completion**: Nette and PHPStan keys, `%parameters%`, `@services`, files in `includes:`.
- **Go to Definition and hover**: includes, `%parameters%`, `@services` and PHP classes,
  across included files.
- **File icon** for `.neon` files.

It follows the [NEON specification](https://github.com/nette/neon-specification) 1.1
(nette/neon 4) and passes its conformance tests. 108 real Nette and PHPStan configs give
no false errors.

## Nette or PHPStan keys?

The plugin decides from the file:

| File | Keys |
| --- | --- |
| name starts with `phpstan`, includes a file with `phpstan` in its path, or has `parametersSchema`, `rules` or `conditionalTags` | PHPStan |
| has a Nette section (`application`, `database`, `di`, `latte`, …) | Nette |
| anything else | both |

## Limits

- Includes are followed up to 8 levels and 32 files. Not followed: `phar://`, paths
  built from parameters (`%rootDir%/…`), `.php` files, files outside the project, files
  over 2 MiB.
- Changes in an included file show after you save it.
- Keys are completed in indented blocks, not inside `{…}`, `[…]` or entity arguments.
- PHP classes come from the IDE's symbol index, so some may be missing while it indexes.
- In files over 1 MiB some tokens aren't highlighted, and files over 2 MiB aren't
  checked for errors (IDE limits).
- To treat other file names as NEON, use Settings › File Types.

## Capabilities

- `project.read` (low risk): reads included files, lists NEON files for `includes:`
  completion and finds PHP classes in the symbol index. Without it the plugin works
  within the open file only.

No network, no processes, no file writes, no stored data.

## Requirements

OpenIDEA 0.1.39 or newer (plugin API 1.3).

## Building

```sh
cargo test
openidea-plugin build    # builds dist/plugin.wasm and checks the package
openidea-plugin link     # loads this folder in OpenIDEA
```

Get `openidea-plugin` from an OpenIDEA release, or install it with
`cargo install --locked --git https://github.com/l0ng-ai/openidea --tag v0.1.39 ide-host --bin openidea-plugin`.

The extra tests need data to run on:

```sh
# NEON specification conformance tests
git clone https://github.com/nette/neon-specification /tmp/neon-spec
git -C /tmp/neon-spec checkout 8e55d78ba2675f60dea13761ad5df56cf55542fc
NEON_SPEC_DIR=/tmp/neon-spec cargo test --test conformance

# your own .neon files: they must parse without errors
NEON_SAMPLES_DIR=/path/to/files cargo test --test samples

# performance on large files
cargo test --release --test performance
```

## License

Apache-2.0, see [LICENSE](LICENSE). The NEON file icon comes from
[intellij-neon](https://github.com/noctud/intellij-neon) (MIT), see [NOTICE.md](NOTICE.md).
