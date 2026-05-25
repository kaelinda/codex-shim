# Icons

This directory intentionally ships **empty**. Tauri requires platform icons at
build time; generate them once with the bundled CLI:

```bash
# Put a 1024x1024 PNG into this directory as source.png, then:
cd ../   # back to tauri-app/src-tauri
npx @tauri-apps/cli icon icons/source.png
```

That writes `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`,
`icon.ico`, plus iOS/Android variants. The file list referenced by
`tauri.conf.json -> bundle.icon` will then resolve.

Alternatively, run the project-local skill `generate-tauri-app-icon` to get a
guided workflow.
