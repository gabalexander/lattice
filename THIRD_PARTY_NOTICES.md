# Third-party notices

lattice adapts code and text from these projects, under their licenses. It also downloads
[mermaid](https://github.com/mermaid-js/mermaid) (MIT, Copyright (c) 2014 - 2022 Knut Sveidqvist), which draws
the diagrams; it isn't part of lattice, and a site `lattice export` writes carries it with its license,
`mermaid.LICENSE.txt`.

## deepwiki-by-cc

https://github.com/andyhtran/deepwiki-by-cc

`src/claude.rs` adapts its Claude Code runner and its sandbox preflight (`src/lib/server/ai/claude-cli.ts` and
`src/lib/server/ai/sandbox-preflight.ts`): the retries of a run that failed for a passing reason, the reading
of the tokens a run used, and the check that Claude can read a file before a long run.

The web app (`web/`) follows its app's flow and components (`RepoInput`, `JobProgress`, `WikiTree`,
`TableOfContents`, `ThemeToggle`, `MermaidDiagram` and its versions with Sync, Resume and Regenerate), and adapts
its fence rules (`src/lib/markdown-fences.ts`, in `web/src/lib/fences.ts`), its mermaid repairs
(`src/lib/mermaid-sanitize.ts`, in `web/src/lib/mermaid-sanitize.ts`) and its renderer's `Sources:` footer and
line-citation pills (`src/lib/wiki-markdown.ts`, in `web/src/lib/markdown.ts`).

```
MIT License

Copyright (c) 2026 Andy Tran

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## crystal

https://github.com/gabalexander/crystal

Much of lattice started in crystal: the modules that say so at their top (`ask`, `claude`, `clipboard`,
`config`, `db`, `download`, `editor`, `export`, `http`, `links`, `mermaid`, `output`, `printable`, `secrets`,
`server`, `shell`), `install.sh`, the workflows, the Makefile and the Homebrew formula. crystal's
`src/mermaid/` is itself adapted from docket's `docket-mermaid` crate, by the same author. The web app's wiki
page (`web/src/lib/wiki/`, `web/src/lib/markdown.ts`, `web/src/lib/mermaid.ts`, `web/mock/synth.mjs`,
`web/scripts/perf.mjs`) is carried over from crystal's (`assets/wiki/`): its layout, its markdown renderer and
tests, mermaid's theme, the lazy drawing and the zoom.

```
MIT License

Copyright (c) 2026 The crystal authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## In the web app

The built web app, which the binary carries, includes these libraries' code.

### Svelte and SvelteKit

https://github.com/sveltejs/svelte and https://github.com/sveltejs/kit

```
Copyright (c) 2016-2025 [Svelte Contributors](https://github.com/sveltejs/svelte/graphs/contributors)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

```
Copyright (c) 2020 [these people](https://github.com/sveltejs/kit/graphs/contributors)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### highlight.js

https://github.com/highlightjs/highlight.js

```
BSD 3-Clause License

Copyright (c) 2006, Ivan Sagalaev.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Material Symbols

https://github.com/google/material-design-icons

The app's icons (`web/src/lib/components/Icon.svelte`) are Material Symbols' outlines, Copyright Google LLC,
under the Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0).

### Google Sans Flex and Google Sans Code

https://github.com/googlefonts/googlesans-flex and https://github.com/googlefonts/googlesans-code

The app's fonts (`web/static/fonts/`), Copyright Google LLC and The Google Sans Code Project Authors, under the
SIL Open Font License, Version 1.1, whose text is beside them: `OFL-google-sans-flex.txt` and
`OFL-google-sans-code.txt`.

