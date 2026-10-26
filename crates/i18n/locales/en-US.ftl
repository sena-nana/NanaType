# Strings both NanaType and NanaType Editor show (nanatype-frame).

## Editor
editor-placeholder = Type your line here… @ for a viewer, / for a marker or effect
editor-source-placeholder = Edit with [b]markup[/b], e.g. [shake]Whoa![/shake][wait=500]
marker-wait = wait { $ms }ms
marker-pause = continue
marker-pause-auto = wait { $ms }ms
marker-page = page
marker-sound = sound { $name }
marker-expression = expr { $name }
marker-portrait = portrait { $name }
marker-shake = shake
marker-vts = VTS { $name }

## Skins
skins-builtin = Built-in
skins-preview-name = Nana

## Windows
windows-input = Input
windows-display = Display
windows-history = History
display-placeholder = Lines appear here · this outline hides once the window is locked
history-placeholder = Chat history appears here · this outline hides once the window is locked

## Common
common-cancel = Cancel

# Bilibili
history-role-host = Host
history-role-viewer = Viewer

# Skin preview editor
skin-edit-invalid = Enter valid numbers and a hex color. Leave fields empty for skin defaults.
skin-preview-loading = Loading preview…
skin-edit-sample = Hello, NanaType! Test wrapping, font size and spacing. 你好，世界！0123456789
    Second line: resize the window to inspect the actual layout.
# History preview, oldest first: host, grouped follow-up, long host line,
# viewer, grouped viewer line, long name, host quoting the viewer, gift, membership.
skin-edit-history-host = Hello! Welcome to the stream.
skin-edit-history-host-more = Tune the [b]font, color and line height[/b] on their own, including a [color=#c45c26]colored[/color] word. [ruby=nana]Nana[/ruby] [at=1001 name="Acher"] [emote=wave alt="wave"]
skin-edit-history-long = A longer host message for wrapping, names and timestamps in a narrow window. 你好！0123456789
skin-edit-history-viewer-name = Mio
skin-edit-history-viewer = Hi! This skin looks great today.
skin-edit-history-viewer-more = A second message in a row stays grouped with the first.
skin-edit-history-long-body = A long name still leaves room for the message, the time and wrapping. 0123456789
skin-edit-history-reply = Got it — this reply quotes that viewer message.
skin-edit-history-fan-name = Hoshi
skin-edit-history-gift = Sent Hearts ×10
skin-edit-history-captain-name = Captain Ren
skin-edit-history-member = Became a member
skin-edit-long-name = A long speaker name for narrow windows
packages-refresh = Reload
packages-installed = Installed
packages-none = No packages yet. Import one to add skins, resources or features.
packages-kind-skin = Skin
packages-kind-resources = Shared resources
packages-kind-function = Functional plugin
packages-kind-sounds = Sound pack

# Package provenance and explicit script consent
package-author = Author
package-created-at = Created on
package-licensed-to = Licensed to
package-license = License
package-signature-valid = Signature valid (author identity unverified)
package-protected-files = protected files
package-unsigned = Unsigned: source and integrity are not authenticated
package-integrity-failed = Integrity check failed; loading refused
package-data-source = Source
package-data-license = License
package-data-protected = covered by the signature
package-data-unprotected = not covered by a signature


# Shared skin parameter controls
skin-params-title = Skin options
skin-params-hint = Appearance options this skin provides. They apply to the whole skin.
skin-params-default = Default
skin-params-invalid-number = Enter a number
skin-params-out-of-range = Must be { $min }–{ $max }
skin-params-invalid-color = Use a #RRGGBB colour
skin-params-invalid-option = Not one of the choices
skin-params-text-too-long = At most { $max } characters
skin-params-text-empty = Can't be empty
skin-params-text-control = Can't contain line breaks or control characters
skin-params-contrast = { $foreground } on { $background } is { $ratio }:1; aim for at least { $min }:1
