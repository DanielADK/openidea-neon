# NEON

Support for NEON, the configuration format of [Nette](https://nette.org) and
[PHPStan](https://phpstan.org): `config/common.neon`, `services.neon`,
`phpstan.neon`, `phpstan.neon.dist`, `phpstan.dist.neon`, `phpstan-baseline.neon`
and any other `*.neon` or `*.neon.dist` file.

- **Highlighting**: keys, strings with their escapes, numbers, dates, `true` /
  `false` / `yes` / `no` / `null`, comments, entities (`Foo(…)`), class names,
  `@service` references, `%parameter%` references, `Class::CONSTANT`,
  `::function()` and punctuation.
- **Structure view**: nested keys; under `services:` each service shows its
  name and the class or factory it is created from; list entries (an
  `ignoreErrors` entry, say) are named by their first value.
- **Folding**: indented blocks, brackets and entity arguments spanning lines,
  multiline strings, runs of comment lines.
- **Problems while typing**: what nette/neon would refuse, with its messages —
  syntax errors, bad indentation, tabs mixed with spaces, unclosed brackets and
  strings, invalid escapes, duplicate keys in one mapping. Unlike nette/neon,
  every problem of the document is shown, not just the first one. Warnings
  mark what nette/neon 3, which PHPStan and most Nette 3 projects use, reads
  differently (see *nette/neon 3* below).
- **Completion**: the keys of the block you are in (top-level sections of
  Nette and PHPStan, the keys of a service definition, PHPStan parameters,
  `ignoreErrors` entries, `parallel`, `excludePaths`, `exceptions`, Nette's
  `application`, `database`, `di`, `latte`, `session`, `tracy`, `http`,
  `mail`, `routing`, `security`, `search`…); values of known keys (`level`,
  booleans, `autowired`); `true` / `false` / `null`; after `%` the parameters
  of the file and the files it includes (nested ones as `a.b`) and the
  built-in ones; after `@` the named services; NEON files of the project in
  `includes:`.
- **Go to Definition**: an `includes:` entry opens the file; `%a.b%` goes to
  the key defining it, in the file or the files it includes; `@name` to the
  service; a PHP class (`App\Model\Foo`, in services, arguments, entities,
  types, `Foo::create`, the keys of PHPStan's `conditionalTags:` and Nette's
  `decorator:`, and `@App\Model\Foo` when no service has that name) to its
  declaration.
- **Hover**: a parameter's value and where it is defined; a service's
  definition; a line of documentation for known Nette and PHPStan keys; where
  a PHP class is declared (also for `@App\Model\Foo`).
- **File icon** for `*.neon` and `*.neon.dist`, whatever icon theme is used.

## Usage

Install the plugin and open a NEON file. There is nothing to set up: the
plugin has no settings. Indentation uses tabs, as Nette does (tab size 4); a
project can still use spaces, NEON allows both as long as one file doesn't
mix them in one block.

### Nette or PHPStan keys

Completion and hovers pick the keys by the file, and PHPStan's files get the
warnings about its nette/neon 3.3:

- a file whose name starts with `phpstan` (`phpstan.neon`, `phpstan.neon.dist`,
  `phpstan.dist.neon`, `phpstan-baseline.neon`) gets PHPStan's keys;
- a file with a `parametersSchema`, `rules` or `conditionalTags` section, or
  including a file with `phpstan` in its path, gets PHPStan's too (an
  extension's `extension.neon`);
- a file with a Nette section (`application`, `database`, `latte`, `tracy`,
  `di`, `search`, …) gets Nette's;
- any other file gets both.

Service definition keys are offered in both. Under `parameters:` only PHPStan
has known keys: Nette's parameters are your own.

### Across files

`includes:` entries are read relative to the including file, as Nette and
PHPStan do, and followed transitively: up to 8 levels and 32 files, each once
(includes that include each other are fine). Definitions are searched in the
order Nette merges them: the file itself, then its includes from the last one
(a later include overrides an earlier one), each followed by what it
includes. Go to Definition lists every definition in that order; the hover
shows the one that wins and says how many it overrides.

Included files are parsed once and kept until their size or modification time
changes: up to 64 files holding up to 8 MiB of text, the least recently used
dropped first. A file is read only when its turn comes, and one call parses at
most 4 MiB of files it doesn't know yet; the rest are read by the next calls,
so no call runs long.

## Behaviour and limits

- **NEON version.** The parser follows the language-neutral
  [NEON specification](https://github.com/nette/neon-specification) 1.1, which
  is what nette/neon 4.0.1 reads: `on` and `off` are strings, `1_000` is a
  string, there is no `\u{…}` escape (only `\uXXXX` and surrogate pairs), a
  value may stand on the line after its key inside brackets, and so may the
  key's colon. It passes the specification's conformance corpus: all 369 valid
  documents parse without a problem, and all 100 invalid ones fail at
  nette/neon's position, 93 with nette/neon's message; the other 7 are a
  bracket not closed at the end of the input, where nette/neon says
  `Unexpected end` and the plugin `Unexpected end: '[' is not closed`. 13 more
  cases are not valid UTF-8, which the IDE never sends. Differential tests
  against nette/neon 4.0.1 on 38,000 generated documents found no document
  one of them accepts and the other refuses.
- **Several problems at once.** After a problem the parser skips the rest of
  the line (or the token, inside brackets) and goes on. A bracket that is never
  closed is taken to end before the next line indented no deeper than the line
  it opened on, so the rest of the file keeps its structure; the problem is
  shown at the opening bracket (mid-file as `Unclosed '[': missing ']'`). A
  multiline string that is never closed (`'''` and Enter, while typing) ends
  the same way. Lines after a `Bad indentation` keep their structure and are
  judged against the block around them. Up to 1,000 problems of the scanner
  and 1,000 of the parser are kept per document, and a run of stray
  characters is one problem.
- **No false positives.** Only what nette/neon itself refuses is reported:
  keys are not checked against the schemas (extensions add their own), and
  parameters or services that can't be found are not reported. 108 real-world
  files (PHPStan's own configuration and baseline, its extensions, Nette's web
  project and libraries, Contributte, baselines of up to 580 KB) give neither
  a problem nor a warning. On the generated documents, every warning about
  nette/neon 3 marks a place nette/neon 3.3 or 3.4 really reads otherwise.
- **File types.** `*.neon` and `*.neon.dist` files (`phpstan.dist.neon` ends
  in `.neon`). Other names can be mapped to NEON in Settings › File Types.
- **Not followed:** `phar://` includes (PHPStan's own `bleedingEdge.neon`),
  includes built from parameters (`%rootDir%/…`), `.php` includes, files
  outside the project, and included files over 2 MiB (a large baseline:
  it defines no parameters you would refer to anyway).
- **Changes in included files** show once they are saved: an included file is
  read again when its size or modification time changes.
- **PHP classes** come from the project's symbol index (no language server
  needed): classes, interfaces, traits and enums named like the reference,
  the ones whose file path matches the namespace best first (PSR-4). Results
  can be missing while the project is being indexed.
- **Completion of keys** works in block notation (indented lines), not inside
  `{…}`, `[…]` or entity arguments; there only `true`, `false` and `null` are
  offered.
- **Large files.** Everything is linear in the text (brackets never closed
  included: 80 KB of them parse in 0.5 ms). Measured natively (release build)
  on a generated 5.2 MB baseline (177,000 lines, 384,000 tokens): scanning and
  parsing 15 ms, highlighting 2.5 ms, structure view 0.3 ms, folding 1.4 ms,
  all three syntax calls of one version together 20 ms; problems, completion
  and hover reuse that parse, whatever the path they are asked for (under 1.3
  ms). WebAssembly is slower: in V8 (Node 22) the first syntax call of a
  version takes 35–37 ms (51–55 ms on the first, cold run), the other two 2–5
  ms, the first completion 1.7 ms. The IDE runs plugins in wasmtime with fuel
  metering, which was not measured here: for a baseline of this size the
  first call of a version is near the 50 ms the IDE waits for. The IDE keeps
  at most 200,000 highlighted tokens per file: documents over 1 MiB get no
  punctuation tokens, and the rest of a bigger one is not highlighted. The
  structure view lists 500 items per list (then "… N more") and 10,000 in
  all. Problems are not checked in files over 2 MiB (an IDE limit).
- **Memory.** Parses are kept for the latest documents, holding up to 8 MiB
  of text together; a larger document is kept alone. A parse takes about five
  times its text: editing a 16 MiB file (the largest the IDE sends) stays at
  116 MiB of plugin memory in V8, a 12 MiB one at 82 MiB (the IDE allows 256
  MiB).

### nette/neon 3

PHPStan 2 bundles nette/neon 3.3; Nette 3 projects mostly have 3.4. These
versions read a few things differently from 4.0.1, so a file this plugin finds
valid can mean something else there, or fail. Those places get a warning:

- in every file, what all of nette/neon 3 reads otherwise: a value on the line
  after its key inside brackets (`{a:` + `b}`: 3.x reads `a` as null and `b`
  as an item of its own), a key's colon on a later line than the key (3.x
  refuses it), and a byte order mark (3.x makes it part of the first key);
- in PHPStan's files (see [Nette or PHPStan keys](#nette-or-phpstan-keys)),
  what only 3.3 reads otherwise: a comma first on its line inside brackets
  and a tab indenting the lines after `- key: value` (both refused), `on` and
  `off` as values (booleans in 3.3), and `\x41` in a double-quoted string
  (read by 3.3, refused since 3.4: a warning there instead of the error).

Not warned about, as real files don't have them: a carriage return without a
line feed (3.x removes it), a date that does not exist (`2024-02-30`, a date
for 3.3, a string since 3.4), integers too big for PHP, and float keys with
15 significant digits ending in 5: 3.x rounds them to 14 digits half to even,
4.0.1 away from zero, so such a key is only reported as a duplicate of the
same number.

## Capabilities

- `project.read` (low risk): reads the files a configuration includes, to find
  the `%parameters%` and `@services` defined there (completion, Go to
  Definition, hover); checks that an included file exists before opening it;
  lists the project's NEON files to complete `includes:` entries; and searches
  the project's symbol index (`Project::symbols`, index only) to go to PHP
  classes. Without it the plugin still works within the open file.

The plugin uses no network, starts no processes, writes no files and keeps no
data.

## Requirements

OpenIDEA 0.1.39 or later (plugin API 1.3: the `diagnostics` and `hover`
providers and the project symbol index are new in 1.3).

## Building

```sh
cargo test
openidea-plugin build    # dist/plugin.wasm, then the same checks as installing
openidea-plugin link     # load this folder in OpenIDEA (Settings › Plugins › Reload after a rebuild)
```

`openidea-plugin` comes with each OpenIDEA release (`openidea-plugin-linux-x86_64.gz`) or from
`cargo install --locked --git https://github.com/l0ng-ai/openidea --tag v0.1.39 ide-host --bin openidea-plugin`.

More tests run when you point them at their data:

```sh
# The NEON specification's conformance corpus (MIT), at the commit nette/neon pins:
git clone https://github.com/nette/neon-specification /tmp/neon-spec
git -C /tmp/neon-spec checkout 8e55d78ba2675f60dea13761ad5df56cf55542fc
NEON_SPEC_DIR=/tmp/neon-spec cargo test --test conformance -- --nocapture

# A folder of real configuration files that must parse without problems:
NEON_SAMPLES_DIR=/path/to/neon/files cargo test --test samples -- --nocapture

# Timings on a generated 5 MB baseline and on brackets never closed, asserted in release builds:
cargo test --release --test performance -- --nocapture
```

The files in `tests/fixtures` are hand-written equivalents of real Nette and
PHPStan configurations.

## Licence

Apache-2.0, see [LICENSE](LICENSE). The NEON file icon comes from
[intellij-neon](https://github.com/noctud/intellij-neon) under the MIT License, see
[NOTICE.md](NOTICE.md).
