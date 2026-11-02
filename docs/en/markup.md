[简体中文](../markup.md) · [English](markup.md) · [日本語](../ja/markup.md)

# Line markup

Markup lives in the input window's editing area; the toolbar and sidebar insert it for you. Parsing never fails: unrecognized tags are shown as plain text and flagged in the status bar. `[[` stands for a literal `[`.

## Styles (paired)

| Markup | Effect |
| --- | --- |
| `[b]…[/b]` `[i]…[/i]` `[u]…[/u]` `[s]…[/s]` | Bold, italic, underline, strikethrough; `[b=0]` cancels a skin's default bold |
| `[color=#e86a92]…[/color]` | Text color: `#rgb` `#rrggbb` `#rrggbbaa` |
| `[size=1.5]…[/size]` `[size=32px]…[/size]` | Font size: a multiple of the skin's size, or pixels |
| `[align=center]…[/align]` | Paragraph alignment: `start` / `center` / `end`, works on empty paragraphs; without it the skin decides |
| `[font=accent]…[/font]` `[font="Noto Serif SC"]…[/font]` | A skin font slot (body / name / accent) or a specific font family |
| `[stroke=#ffffff,2]…[/stroke]` | Outline: color, width |
| `[shadow=#000000aa,2,2,4]…[/shadow]` | Shadow: color, x, y, blur |
| `[ruby=かんじ]漢字[/ruby]` | Ruby (reading annotation) |

## Per-character effects (paired)

| Markup | Effect |
| --- | --- |
| `[shake]` `[wave]` `[jump]` `[rainbow]` `[pulse]` `[flicker]` | Shake, wave, jump, rainbow, pulse, flicker; accept `amp=` and `speed=` (multiples of the skin's defaults) |
| `[fade]` `[pop]` `[slide]` | Entrances: fade in, pop, slide up; `[pop=120]` sets the duration in milliseconds. A line has only one entrance: whichever covers the most characters wins and applies to the whole line, overriding settings and the skin |


Ongoing effects can be nested, e.g. `[wave][rainbow]波浪彩虹[/rainbow][/wave]`. A segment keeps all its effects; `amp=0` turns that effect off, and reduced-motion mode turns off every ongoing effect.

## Rhythm

| Markup | Effect |
| --- | --- |
| `[speed=0.5]…[/speed]` | Local speed change: a multiplier (0.5 means twice as fast) |
| `[speed=40ms]…[/speed]` | A fixed number of milliseconds per character |
| `[instant]…[/instant]` | The whole segment appears at once |
| `[em]…[/em]` | Emphasis: bold plus letter spacing, with a 100 ms pause before and after each character, one beat per character; `[em=200]` sets the milliseconds on each side (spaces don't pause) |
| `[wait=500]` | Pause for 500 milliseconds |
| `[pause]` | Stop and wait for the continue key |
| `[pause auto=2000]` | Stop, then continue automatically after 2 seconds |
| `[page]` | Wait for continue, then clear the display and carry on from the top of the box |

When a line is taller than the display, the skin's `overflow` decides what happens (overridable in **Settings › Skins › When text overflows**): `scroll` scrolls everything up a line once typing reaches the bottom of the box (default), `page` waits for continue after each full page and then turns the page (as if `[page]` were inserted automatically), and `clip` simply cuts it off.

## Sounds, expressions and the screen

| Markup | Effect |
| --- | --- |
| `[se=coin]` `[se=coin vol=0.6]` | Play the skin's sound effect named `coin` |
| `[expr=smile]` | Trigger an expression: switch to the portrait variant of the same name and send it to VTube Studio / OSC / WebSocket. In VTube Studio this activates the expression bound to the hotkey of the same name (or the one mapped in settings) |
| `[portrait=happy]` | Only switch the portrait variant |
| `[screenshake=6,300]` | Shake the display: amplitude, milliseconds |
| `[sticker=cats/hello h=1.6]` | Insert `hello` from sticker library `cats`, 1.6 times the font size tall |
| `[emote=wave]` | Insert the skin's own emote `emotes/wave` |
| `[img="file:C:/pic.png" alt="图"]` | Insert an image |
| `[at=1234 name="观众" avatar=blob:bili-face-1234]` | Mention a viewer from the live room: shown as a tag with avatar and nickname (styled by the skin's `[mention]`), counted as a single character. `at` is the Bilibili uid and may be omitted; `avatar` may be omitted. Usually inserted by the chat panel's "@" button; "Reply" only attaches a quote of the original message |

## VTube Studio

Each of these is sent when typing reaches its position; when you skip, the markers inside the skipped part are sent immediately.

| Markup | Effect |
| --- | --- |
| `[vts hotkey=挥手]` | Press a hotkey of the current model (by name or ID) |
| `[vts expr=Smile.exp3.json]` | Turn on an expression file |
| `[vts expr=Smile.exp3.json active=off fade=0.3]` | Turn off an expression file, fading out over 0.3 s |
| `[vts param=MouthSmile value=0.8 ms=1500]` | Set a parameter to 0.8 and hold it for 1.5 s (omit `ms` to set it once) |

In the integration settings you can also turn on lip sync (drives `MouthOpen` and similar parameters while typing, closing the mouth during pauses), and reset the expressions a line turned on when the line leaves the display or the next line starts.

## Whole-line settings

Written at the very start:

```text
[@auto=1500 after=fade delay=800 per=20]
```

- `auto`: how many milliseconds after playback finishes to advance to the next line automatically (in queue mode).
- `after`: `hold` to keep it, `fade` to fade out (`delay` is how long it stays in milliseconds, `per` adds milliseconds per character), `clear` to remove it immediately.
- `speaker`: say this one line as another character (its name, avatar, colour and voice), leaving the active character as it is. Give the character's name (matched as written, then ignoring case; quote it when it has spaces: `[@speaker="Guest Star"]`) or its number in the list under **Settings › General › Characters**, from 1 (`[@speaker=2]`). One not found leaves the active character. It can also be written on its own as `[speaker=Nana]`, likewise only at the very start.

## Quick phrase templates

Quick phrases can use `{{name}}` (character name) and `{{time}}` (current time).

The sidebar on the right of the input window has an icon bar that switches between four panels: **Controls**, **Stickers**, **Quick phrases** and **Chat**; click the current icon again to collapse the sidebar. The **Controls** panel turns the rhythm, screen and sound, and expression markup above into the same chips you see in the editor — click one to insert it at the caret. Text effects, entrances and speed are marked with brackets and apply to the selection. Hovering a tile shows what it does and its markup source, and there is a search box at the top. Chat messages can be pinned to the upper half of the sidebar, and the divider between them and the panel below can be dragged.

The toolbar only handles how text looks: bold, italic, underline, strikethrough; color, size, font, alignment; outline, ruby and clear formatting. It provides panels for paragraph alignment, font size and font. Alignment applies to the paragraph under the caret or the selected paragraphs; size can be a multiple of the skin's size (0.5–3×) or a fixed pixel size (4–400px). Fonts can use the skin's body, name and accent slots, or a font family name you type. The panels show the current properties, and **Mixed** when they differ across the selection.

**Skin default** clears the corresponding override; in source you can use `[size=inherit]…[/size]` and `[font=inherit]…[/font]` to restore inheritance inside an outer style. Multipliers and font slots follow the skin, while pixel sizes and named font families stay fixed. Clearing formatting also restores the default alignment of the affected paragraphs.
