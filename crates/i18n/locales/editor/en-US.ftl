# Strings only NanaType Editor shows (feature `editor`).

## Editor window
editor-app-title = NanaType Editor
editor-app-subtitle = Create, edit, sign and debug NanaType packages. An exported .ntpkg takes effect once it is imported into NanaType.
editor-data-dir = Data folder: { $path }
editor-data-dir-open = Open

# Package provenance and explicit script consent
pkgedit-title = Package editor
pkgedit-intro = Changes stay in a draft until you export a .ntpkg and import it.
pkgedit-kind-skin = Skin only
pkgedit-kind-resources = Resources only
pkgedit-kind-lua = Lua only
pkgedit-kind-sounds = Sound pack only
pkgedit-kind-mixed = Mixed
pkgedit-id-placeholder = Package ID, e.g. my.package
pkgedit-new = New
pkgedit-open-file = Open .ntpkg…
pkgedit-drafts = Drafts
pkgedit-drafts-none = No saved drafts yet.
pkgedit-open = Open
pkgedit-discard = Discard draft
pkgedit-discard-question = Delete this draft permanently? Installed packages are not affected.
pkgedit-edit = Edit
pkgedit-copy = Copy as new package
pkgedit-origin-new = New project
pkgedit-origin-installed = Opened from installed package
pkgedit-origin-archive = Opened from file
pkgedit-origin-copy = Derived copy of
pkgedit-derived-from = Derived from
pkgedit-derived-unsigned = the original signature was removed; this copy does not speak for the original author
pkgedit-unsaved = Unsaved changes
pkgedit-saved-state = Draft saved
pkgedit-not-stored = Unchanged
pkgedit-validating = Checking in the background…
pkgedit-valid = No errors
pkgedit-errors = { $count ->
    [one] 1 error
   *[other] { $count } errors
}
pkgedit-save = Save draft
pkgedit-revert = Revert to last save
pkgedit-export = Export .ntpkg…
pkgedit-close = Close
pkgedit-close-unsaved = There are unsaved changes. Save them as a draft before closing?
pkgedit-close-save = Save and close
pkgedit-close-discard = Close without saving
pkgedit-tab-files = Files
pkgedit-tab-package = Package
pkgedit-tab-signing = Protection & signing
pkgedit-tab-preview = Preview
pkgedit-import = Import asset…
pkgedit-remove = Remove file
pkgedit-remove-question = Remove this file from the draft?
pkgedit-read-only = Read-only
pkgedit-format = Format
pkgedit-search = Search in file
pkgedit-search-next = Next
pkgedit-search-none = No matches
pkgedit-no-file = Choose a file on the left.
pkgedit-binary = File details
pkgedit-file-format = Format
pkgedit-file-size = Size
pkgedit-file-dependency = From dependency (read-only)
pkgedit-data-declared = Declared as data
pkgedit-problems = Problems (click to locate)
pkgedit-no-problems = No problems found.
pkgedit-manifest-invalid = package.toml cannot be parsed; fix it under Files first.
pkgedit-manifest-protected = package.toml is protected by the signature and is read-only. Create a derived copy to change it.
pkgedit-version = Version
pkgedit-capabilities = Capabilities (enabling skin or Lua adds starter files)
pkgedit-dependencies-hint = Dependencies, one "package-id@exact-version" per line; applying bundles them from installed packages.
pkgedit-apply = Apply
pkgedit-data = Declared data
pkgedit-data-none = No data declared.
pkgedit-pipelines = Pipelines
pkgedit-pipelines-none = No pipelines declared.
pkgedit-data-missing = file missing
pkgedit-data-add = Add data
pkgedit-data-id = Data ID
pkgedit-data-path = File path
pkgedit-data-format = Format
pkgedit-data-version = Format version
pkgedit-data-constraints = JSON constraints
pkgedit-data-constraints-hint = JSON constraints are optional and apply to JSON data only, for example {"{"}"type": "object", "required": ["items"]{"}"}. Leave empty for no limits.
pkgedit-entry-edit = Edit
pkgedit-entry-remove = Remove
pkgedit-entry-remove-question = Remove this declaration?
pkgedit-pipeline-add = Add pipeline
pkgedit-pipeline-name = Pipeline name
pkgedit-pipeline-processor = Processor
pkgedit-pipeline-inputs = Inputs: a data ID (pkg:package-id/data-id for a dependency), or pipeline.output to feed from another pipeline.
pkgedit-pipeline-params = Parameters, one name = number per line
pkgedit-pipeline-outputs = Outputs
pkgedit-pipeline-unknown = This processor is not registered; edit it in the source.
pkgedit-pipeline-broken = This declaration does not parse; edit it in skin.toml.
pkgedit-pipelines-no-skin = Pipelines live in skin.toml; enable the skin capability to declare them.
pkgedit-skin-protected = skin.toml is protected by the signature and is read-only. Create a derived copy to change it.
pkgedit-seal-note = A signature only shows that protected files match what was signed; it does not prove who the author is. Editable files may change without invalidating it.
pkgedit-key-load = Use this computer's signing key
pkgedit-key-none = This computer has no signing key yet.
pkgedit-key-create = Create signing key
pkgedit-key-fingerprint = Signing key fingerprint
pkgedit-key-note = The private key stays in the system's protected storage (macOS Keychain, Windows DPAPI). It never goes into a package, a log, the resource interface or a plugin process.
pkgedit-key-unavailable = The signing key is unavailable
pkgedit-protect = Choose the files to protect. package.toml is always protected; once signed, protected files are read-only and the file list is fixed.
pkgedit-sign = Sign
pkgedit-signed-own = Signed with this computer's key. Remove the signature to edit, then sign again.
pkgedit-signed-other = Signed with another key; protected files are read-only. To change them, create a derived copy without the original signature.
pkgedit-unseal = Remove signature to edit
pkgedit-derive = Create derived copy
pkgedit-derive-hint = A derived copy gets a new package ID, records its source and drops the signature.
pkgedit-preview-none = This project has no skin, or the skin has not passed its checks yet.
pkgedit-debug-no-lua = This project does not declare the Lua capability.
pkgedit-debug-risk = Debugging runs the draft's script in a separate sandboxed worker process. It only receives the test events you send, and its results only appear here, but a script can still show misleading content or use resources. Only run source you have reviewed. Editing any file stops the script and needs a new confirmation.
pkgedit-debug-trust = I reviewed the current source and trust it to run in the debug process
pkgedit-debug-start = Start
pkgedit-debug-stop = Stop
pkgedit-debug-clear = Clear results
pkgedit-debug-events = Send a test event
pkgedit-debug-send = Send
pkgedit-debug-invalid-json = An event must be a JSON object with a string "type" field.
pkgedit-debug-error = Script error
pkgedit-debug-output = Returned views
pkgedit-debug-waiting = Waiting for the script's view…
pkgedit-debug-idle = The script is not running.
pkgedit-debug-log = Log
pkgedit-saved = Draft saved
pkgedit-unsaved-open = The open project has unsaved changes. Save or close it first.
pkgedit-open-failed = Could not open the project
pkgedit-exported = Exported
pkgedit-export-failed = Export failed
pkgedit-debug-locate = Go to

pkgedit-undo = Undo

pkgedit-redo = Redo

pkgedit-discard-form = Discard form changes

pkgedit-recovery = Recovery available

pkgedit-recover = Recover

pkgedit-width = Width

pkgedit-height = Height

pkgedit-simulate = Simulate NanaType personalization

pkgedit-landscape = Landscape

pkgedit-portrait = Portrait

pkgedit-narrow = Narrow

pkgedit-default-size = Default size

pkgedit-static = Still / stop

pkgedit-play = Play

pkgedit-reduced = Reduced motion

pkgedit-effects-off = Advanced effects off

pkgedit-replay = Replay

pkgedit-fit = Fit

pkgedit-checker = Transparency grid

pkgedit-white = White background

pkgedit-dark = Dark background

pkgedit-speaker = Sample speaker

pkgedit-preview-stale = Current content has not passed validation. Showing the last valid preview.

pkgedit-preview-size-error = Enter logical dimensions: width 160–8192, height 96–8192

pkgedit-operation-stale = The project changed. Apply the operation again.


pkgedit-property-select = Select a declaration group

pkgedit-up = Up

pkgedit-add-property-hint = Add a field by name and TOML value; use inline tables for groups and arrays for lists.

pkgedit-property-name = Field name

pkgedit-add = Add



pkgedit-inherit = Restore inheritance

pkgedit-unbind = Explicitly unlink parameter

pkgedit-bound-warning = This value is parameter-bound. Explicitly unlink it before replacing it.

pkgedit-accept-disk = Accept disk version

pkgedit-add-decor = Add decoration

pkgedit-base = Base skin



pkgedit-cover = Cover path

pkgedit-description = Description

pkgedit-disk-conflict = Disk conflict

pkgedit-duplicate = Duplicate element

pkgedit-event-sequence = Test event sequence (JSON array, up to 32)

pkgedit-file-target = Package path to create or move to

pkgedit-keep-local = Keep local changes

pkgedit-layer-down = Move down a layer

pkgedit-layer-up = Move up a layer

pkgedit-layout-base = Base layout

pkgedit-layout-rule = Breakpoint rule

pkgedit-log-filter = Filter logs

pkgedit-min-app = Minimum app version

pkgedit-name = Name

pkgedit-new-file = New text file

pkgedit-open-directory = Open project directory…

pkgedit-origin-directory = Directory project

pkgedit-reference-auto = Manifest reference; updated automatically

pkgedit-reference-manual = Program reference; check manually

pkgedit-rename = Rename / move

pkgedit-rename-question = Change the file path and the references below? Check program references manually.

pkgedit-replace = Replace file…

pkgedit-replace-all = Replace all

pkgedit-replace-one = Replace current match

pkgedit-replacement = Replace with

pkgedit-save-copy = Save local copy…

pkgedit-search-previous = Previous

pkgedit-tags = Tags (comma-separated)

pkgedit-unused-asset = No recognized references

pkgedit-tab-sounds = Sounds

pkgedit-add-responsive = Create responsive layouts

pkgedit-template = Use template

pkgedit-property-exists = Enter a field name that is not already declared

pkgedit-reload-directory = Reload directory project

pkgedit-source-too-large = Source exceeds the editing limit (1 MiB).

pkgedit-layout-matched = Matching layers; later layers override earlier ones

pkgedit-bind = Bind parameter

pkgedit-pending-forms = Apply or discard the pending form changes first.

pkgedit-mode-design = Design
pkgedit-mode-data = Data and pipelines
pkgedit-mode-lua = Lua
pkgedit-project-menu = Project menu
pkgedit-toolbar = Editor toolbar
pkgedit-status = Project status
pkgedit-split-source = Show source beside the canvas
pkgedit-files-preview = Show live preview
pkgedit-drawer-close = Close panel
pkgedit-custom-size = Custom size
pkgedit-stage-settings = Canvas settings
pkgedit-stage-size = Preview size in logical pixels (width × height)
pkgedit-params-none = This skin declares no parameters.
pkgedit-preview-params = Preview parameters
pkgedit-layers = Layers
pkgedit-canvas = Canvas
pkgedit-inspector = Inspector
pkgedit-add-field = Add field
pkgedit-section-source = This table's TOML
pkgedit-section-reveal = Show in source
pkgedit-section-inline = This table has no header of its own; edit it in the table that contains it.
pkgedit-file-operations = New, move and replace
pkgedit-files-none = No files of this kind.
pkgedit-lua-sources = Lua sources
pkgedit-sound-files = Audio files
pkgedit-sound-declarations = Sound declarations
pkgedit-layer-dialogue = Dialogue
pkgedit-layer-nameplate = Name tag
pkgedit-layer-portrait = Portrait
pkgedit-layer-continue = Continue mark
pkgedit-layer-text = Text
pkgedit-layer-decor = Decorations
pkgedit-layer-history = History
pkgedit-layer-chrome = Chrome
pkgedit-layer-effects = Effects
pkgedit-layer-fonts = Fonts
pkgedit-layer-materials = Materials
pkgedit-layer-mention = Mentions
pkgedit-layer-params = Parameters
pkgedit-layer-param_groups = Parameter groups
pkgedit-layer-presets = Presets
pkgedit-layer-shaders = Shaders
pkgedit-layer-sounds = Sounds
pkgedit-layer-responsive = Responsive
pkgedit-layer-pipelines = Pipelines
pkgedit-layer-frame = Frame
pkgedit-layer-remove = Remove decoration
