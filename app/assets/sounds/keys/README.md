# Keystroke sounds

Clicks for keys pressed in the input window (`app/src/services/sounds.rs`),
one folder per set, chosen in Settings › Playback › Sound. Normal keys pick one
of `key-*.wav` at random, never the same one twice in a row; Enter plays
`enter.wav`. Neither set has a Backspace recording, so Backspace/Delete replay
one of them at a lower pitch.

All files were taken unmodified, apart from their names, from
[Tickeys](https://github.com/yingDev/Tickeys)
`Tickeys.app/Contents/Resources/data/<set>/`, which ships each set with its
Freesound notice. Provenance and licence texts are in `packaging/licenses/`.

## `mechanical/` — CC0 1.0

A 1986 DOS mechanical keyboard, recorded by
[jim-ph](https://freesound.org/people/jim-ph/packs/12363/).
Backspace: `enter.wav` at 0.88×. Licence: `KeySounds-CC0.txt`.

| File | Tickeys file | Freesound | SHA-256 |
| --- | --- | --- | --- |
| `key-1.wav` | `1.wav` | Vintage Keyboard 1–4 (194795–194798) | `715d306d26a653d18f91ac336e4ac612a28a58cf648d3d4043e13057c777370a` |
| `key-2.wav` | `2.wav` | 〃 | `a5fcf7bae96a89490a15c0ba6b2ff4eb6f7427f24d986d35b34c2c622e595c6f` |
| `key-3.wav` | `3.wav` | 〃 | `bbafed79ded8d3d3d871bf96fe99eb33ea00af38875307fc728d275d05f6e752` |
| `key-4.wav` | `4.wav` | 〃 | `834c687e479025853b58596a98db45e9abae76821d8f8f0b7f4b95aec46f3974` |
| `enter.wav` | `5.wav` | keyboard5 (194799) | `b498a8873d6d83c432898d39ccd151a1bcaae1054377303d7dcc79bd46a9be1d` |

## `bubble/` — CC BY 3.0, attribution required

Bubbles by [Glaneur de sons](https://freesound.org/people/Glaneur%20de%20sons/packs/6686/)
(pack 6686, sounds 104940–104950); Tickeys does not record which original each
file is. Backspace: `key-7.wav` (the shortest pop) at 0.8×.
Licence: `KeySounds-Bubble-CC-BY-3.0.txt`.

| File | Tickeys file | SHA-256 |
| --- | --- | --- |
| `key-1.wav` | `1.wav` | `d93d8059825d1b4d4c3d80b1740ea352c8f1c9553eb0ce5602abff9a17eb14ef` |
| `key-2.wav` | `2.wav` | `0bcb0532954ddcb286184f24749bf9633beae1b4e2c76112df97f037cff2a33f` |
| `key-3.wav` | `3.wav` | `2b92eed02a67df671b756873041d012b3378ac79ba47802486302b82271f6478` |
| `key-4.wav` | `4.wav` | `fa1f106ec14a3b852a47bb555ea84e494f36109fbd61afaf95d9a02451c52c5e` |
| `key-5.wav` | `5.wav` | `e8484874c8fe79326b9c6287866f5e57f5501ebfa4cfe31a41ecc0a49dd06efe` |
| `key-6.wav` | `6.wav` | `b710d1236d49f61b4672f646f39942540b82a175da2c009269f77ad8cf2b74bf` |
| `key-7.wav` | `7.wav` | `88e696519b9a77a64aa54c651fe9ea784dc9706c3a1fcd9281b6c3f056a6d2e8` |
| `key-8.wav` | `8.wav` | `131f063031d07bc66b3175c8db31e84da51f489f76b7000a010a7651af7550c1` |
| `enter.wav` | `enter.wav` | `7f8c18095e7a57335ef1e4360cebe121830b93a5216f54ced9cc75999daff377` |
