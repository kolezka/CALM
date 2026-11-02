# Third-party notices outside Cargo

`gmake notices` builds `THIRD_PARTY_NOTICES` from two parts: the Rust crates (cargo about)
and the files in `notices/`, which cover what cargo cannot see:

- Native code that gpui-libghostty builds with Zig and links statically into `calm-app`:
  Ghostty and its C/C++ and Zig dependencies, the Zig standard library, and the two fonts
  Ghostty embeds (JetBrains Mono, Symbols Nerd Font).
- npm packages bundled into first-party plugins that ship in the app (Preact in `github-prs`).

Each file starts with the component name, its upstream URL and the license used. The text
below that is copied verbatim from the package source.

## How the list was checked (gpui-libghostty 0.3.1, Zig 0.16.0)

- C/C++ libraries: symbols present in the release `calm-app` binary (`nm`). FreeType, libpng,
  zlib, Oniguruma, Dear ImGui, Highway, simdutf, glslang, SPIRV-Cross and Wuffs all had
  matches.
- Fonts: their UTF-16 names appear in the release binary.
- Zig packages: their symbols are not named in the binary, so every package the Ghostty build
  fetched is listed except build-only tools (aro, translate_c), test images and themes.
- GNU libintl (LGPL) is compiled into `libghostty-internal.a` but `nm` finds none of its
  symbols in `calm-app`, so the linker drops it. Recheck this after every gpui-libghostty
  upgrade: if libintl shows up in the binary, LGPL terms apply to the shipped app.

## Updating

After a gpui-libghostty, Zig or plugin dependency upgrade, recheck the list above, refresh
the files from the new package sources (Zig packages live in
`~/Library/Caches/gpui-libghostty/zig-global/p`), then run `gmake notices`.
