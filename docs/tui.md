# Terminal UI

MatchBox can draw full-screen terminal applications. This page is the API contract: the JVM BoxLang implementation is
meant to offer the same calls, so a program written for one runs on the other.

Build with the `bif-tui` feature (`cargo build --features bif-tui`); native programs (`--target native`) include it when
MatchBox was built with it.

```boxlang
tui = createObject( "rust", "tui.TUI" );
tui.init();                       // raw mode, alternate screen, mouse
try {
    while ( true ) {
        size = tui.size();        // { width, height }
        tui.begin_frame();
        tui.fill( 0, 0, size.width, size.height, { bg: "#303446" } );
        tui.border( 2, 1, 30, 5, { border: "rounded", fg: "#51576d", bg: "#292c3c", title: "Steps", titleFg: "#00ff78" } );
        tui.gradient( 4, 3, "BxCachy", "#00dbff", "#00ff78", { bold: true } );
        tui.end_frame();
        event = tui.poll_event( 50 );
        if ( !isNull( event ) && event.type == "key" && event.key == "Ctrl+c" ) break;
    }
} finally {
    tui.shutdown();
}
```

## Frame

| Call | Does |
|---|---|
| `init()` / `shutdown()` | Enter and leave the full-screen terminal (raw mode, alternate screen, mouse capture). Always call `shutdown()`. |
| `begin_frame()` | Starts a frame; clears everything drawn since the last one. |
| `end_frame()` | Paints the frame on the terminal (only changed cells are written). |
| `size()` | `{ width, height }` of the terminal. |
| `render_to_string( width, height )` | The current frame drawn into memory as plain text, one line per row. Needs no terminal: use it to test a UI. |
| `set_mouse( enabled )` | Turns mouse reporting on or off. |

## Drawing

Commands draw in call order; later commands paint over earlier ones. Anything outside the screen is clipped.

| Call | Does |
|---|---|
| `fill( x, y, width, height, style )` | Paints a rectangle with `style.bg` and clears its text. |
| `text( x, y, text, style )` | Writes text at a position. |
| `gradient( x, y, text, from, to, style )` | Text whose colour blends from `from` to `to` across its width. |
| `border( x, y, width, height, options )` | A bordered box. `options` is a style plus `border` (`rounded` default, `plain`, `double`, `thick`), `title`, `titleFg`. The inside is filled when `bg` is set. |
| `print( x, y, text, color, bold )` | Shorthand for `text` with a foreground colour. |
| `image( x, y, width, height, path [, options] )` | Draws a picture file (PNG, JPEG, GIF first frame, WebP) inside the rectangle of cells. Returns `true`, or `false` (and draws nothing) when the file is missing or is not a picture. See [Images](#images). |
| `image_support()` | How a picture is drawn with `protocol: "auto"` here: `"kitty"` or `"halfblocks"`. Read from the environment; the terminal is never asked. |

A **style** is a struct (or `{}`) with any of `fg`, `bg` (colours), `bold`, `dim`, `italic`, `underline` (booleans).
Fields left out keep what is already in the cell, so a background painted by `fill` shows behind later text.

## Images

`image( x, y, width, height, path [, options] )` puts a picture in a rectangle of cells (`width` x `height`; a cell is taken as one pixel wide and two high). `options` is a struct (or leave it out):

| Option | Values | Default |
|---|---|---|
| `fit` | `"contain"`: the whole picture, as large as fits, centred; what is left of the rectangle is untouched. `"cover"`: the rectangle is filled and the overflow is cut off, around the centre. | `"contain"` |
| `protocol` | `"auto"`, `"kitty"`, `"halfblocks"`; `"sixel"` and `"iterm2"` are accepted names that are drawn as half blocks for now. An unknown name is an error. | `"auto"` |

How it is drawn:

* **Half blocks.** Every cell holds two stacked pixels, drawn with `▀` (foreground = upper pixel, background = lower pixel) in true colour. It works in every terminal. Pixels that are more than half transparent are left out and show what was in the cell. This is also what `render_to_string` returns for every picture, whatever the protocol, so a UI with pictures can be tested: a picture drawn at its own pixel size (width x 2 x height) appears pixel for pixel.
* **Kitty graphics.** The picture is sent to the terminal once (as PNG, in chunks), then placed over the cells with a short escape sequence; `cover` uses the protocol's own cropping, `contain` places the picture at its aspect, taking a cell as 1:2. Placements are sent only when they change from the last frame (no flicker), and removed when the picture goes away or `shutdown()` runs. The cells under the picture are not painted.

`protocol: "auto"` reads the environment and never asks the terminal (a reply would arrive as input and could block):

| Environment | Result |
|---|---|
| `MATCHBOX_TUI_IMAGES=kitty` or `halfblocks` | that one (wins over everything below) |
| `TMUX` or `STY` set (tmux, screen) | half blocks: graphics need passthrough, which is not done |
| `KITTY_WINDOW_ID`, `GHOSTTY_RESOURCES_DIR`, `TERM_PROGRAM=ghostty`, `TERM=xterm-kitty` or `xterm-ghostty` | Kitty |
| anything else | half blocks |

Compatibility. Only what was run is marked verified:

| Terminal | Drawn as | Status |
|---|---|---|
| any terminal with true colour | half blocks | verified by tests and by capturing the output of a real run in a pseudo-terminal |
| Kitty, Ghostty | Kitty graphics | the escape sequences are covered by tests and follow the protocol documentation; **not yet run in a real Kitty or Ghostty** |
| Konsole | half blocks by default | graphics support unverified |
| WezTerm, foot | half blocks by default (force `kitty` or wait for Sixel at your own risk) | unverified |
| tmux, screen | half blocks | by design |

### Bad pictures

`image` is built so that a picture cannot take the program down. It returns `false` and draws nothing for a file that is missing, is a folder, is empty, is not a picture, is damaged or truncated, is over a limit, or makes the decoder panic; the frame it is part of is drawn normally and the next frame draws as usual.

* **Limits, checked before decoding.** A file larger than 20 MB, or whose header says it has more than 12 megapixels, is refused without decoding it (the dimensions are read from the header only, so a few bytes claiming ten billion pixels cost nothing). Change them with `MATCHBOX_TUI_IMAGE_MAX_BYTES` and `MATCHBOX_TUI_IMAGE_MAX_PIXELS` (positive whole numbers). The decoder is also given an allocation limit. A rectangle wider or taller than 1024 cells is refused.
* **Time.** A decode within the limits is bounded: on a release build a 12 megapixel PNG takes about 0.4 s (a test measures it), smaller pictures much less, and a 20 megapixel one would take about 0.6 s, which is why the default limit is 12. The decode happens on the first call that draws the file, on the calling thread, and is not run in the background. Pictures longer than 2048 pixels on a side are reduced to that before they are kept.
* **Memory.** Decoded pictures are kept for later frames, at most 48 and 64 MB, resampled grids at most 256 and 32 MB; the oldest go first. A file that is edited on disk is read again.
* **Failures are remembered.** A file that failed (or was over a limit) is remembered by path, size and modification time, so a screen that draws it every frame does not decode it 20 times a second; changing the file tries it again.
* **Panics.** Every decode, resample and draw runs under `catch_unwind`; a panic is the same as a bad file, and the panic message is not printed (it would wreck a full-screen display). **This holds only in builds that unwind.** The release profile of the `matchbox` binary and of the native runner stub is built with `panic = "abort"`, where the first panic ends the process whatever the code does; there the protection against a decoder panic is the limits and the header checks above, plus the fact that the decoders are fuzzed. Building the runner with `panic = "unwind"` makes the guard effective in native programs at some cost in size.

## Colours

`"#RRGGBB"`, `"#RGB"`, `"rgb(r, g, b)"`, the 16 ANSI names (`black red green yellow blue magenta cyan white gray darkgray
lightred lightgreen lightyellow lightblue lightmagenta lightcyan`), and `default` (the terminal's own). A text that is
none of these is an error, not a silent white.

True colour is used when the terminal says it supports it (`COLORTERM=truecolor` or `24bit`); otherwise every colour
is mapped to the nearest of the 256-colour palette, so the same program looks right (if less smooth) on any terminal.

## Events

`poll_event( timeoutMs )` returns a struct, or null when nothing arrives in time:

| `type` | Fields |
|---|---|
| `key` | `key`, `ctrl`, `alt`, `shift`. `key` is the character typed (`a`, `A`, `?`) or a name: `Enter Tab BackTab Backspace Escape Left Right Up Down Home End PageUp PageDown Delete Insert F1`..., with `Ctrl+` / `Alt+` in front when held (`Ctrl+c`, `Alt+x`) and `Shift+` in front of named keys. |
| `click` | `x`, `y` (left button), and `widgetId` when a widget was hit. |
| `scroll` | `direction` (`up` or `down`), `x`, `y`. |
| `resize` | `width`, `height`. |

## Widgets

The retained-mode widgets (`tui.Text`, `tui.List`, `tui.Table`, `tui.Block`, `tui.Input`, `tui.Button`, `tui.VBox`,
`tui.HBox`, `tui.ProgressBar`, `tui.Custom`, drawn with `render_widget`) still exist; they take the same colours. The
canvas draws after them. A hand-drawn UI (the BxCachy setup wizard) uses the canvas only.

## Notes for the JVM implementation

- Same method names, argument order, style fields, colour syntax and event shapes as above.
- `render_to_string` is part of the contract so UI code can be unit-tested identically on both runtimes.
- Truecolor detection and the 256-colour fallback behave the same.
- `image` and `image_support` are part of the contract: the same options, the same `render_to_string` half-block output, the same environment rules.
