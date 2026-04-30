# FORMATS.md

## Status

Draft specification.

This document defines artifact and media formats in Kairo, and how they map to
executors, viewers, and tools.

---

## 1. Purpose

Formats answer:

> What is this artifact, and how can it be used or displayed?

Formats are used to:

- classify artifacts
- select appropriate viewers/executors
- inform UI behavior
- support reproducibility

---

## 2. Core Principle

```
Format = description of data
Environment = how it runs
Executor = how it is executed
Viewer = how it is displayed
```

---

## 3. Format Descriptor

```ts
type FormatDescriptor = {
  kind: string
  media_type?: string
  extension?: string
  metadata?: Record<string, any>
}
```

---

## 4. Standard Format Kinds

### 4.1 Binary Executable

```
kind: "executable"
```

Examples:

- .exe
- ELF

---

### 4.2 Source Code

```
kind: "source"
```

Examples:

- .c, .ts, .py

---

### 4.3 Archive

```
kind: "archive"
```

Examples:

- zip
- tar

---

### 4.4 Media

```
kind: "image"
kind: "audio"
kind: "video"
```

---

### 4.5 Data

```
kind: "data"
```

Examples:

- JSON
- CSV

---

### 4.6 Web Content

```
kind: "web"
```

Examples:

- HTML
- JS bundles

---

### 4.7 Disk Image

```
kind: "disk_image"
```

Examples:

- .img
- .iso

---

## 5. Format Detection

Formats may be determined by:

- explicit metadata
- file extension
- MIME/media type
- content sniffing (optional)

Explicit metadata should take precedence.

---

## 6. Viewer Mapping

Formats map to viewers:

```json
{
  "kind": "image",
  "viewer": "image_viewer"
}
```

Examples:

- image → image viewer
- web → browser executor
- executable → runtime executor
- data → table/graph viewer

---

## 7. Executor Mapping

Formats may imply executor requirements:

- executable → emulator/container/native
- web → browser
- disk_image → emulator/VM

---

## 8. API Representation

```json
{
  "artifact": {
    "format": {
      "kind": "image",
      "media_type": "image/png"
    }
  }
}
```

---

## 9. Reproducibility

Formats must be recorded in execution records.

---

## 10. Implementation Checklist

1. Format descriptor type
2. Detection logic
3. Viewer mapping
4. Executor mapping
5. API exposure

---

End of FORMATS.md
