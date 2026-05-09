export type BlobPreviewProps = {
  blobId: string;
};

/**
 * Slice 5 placeholder. Slice 10 ships a registry-driven viewer
 * that picks text vs binary based on a content sniff. Active /
 * unsafe content (HTML, JS, emulators) requires a daemon-
 * approved sandbox per `WEB_CLIENT.md` §12.2 and is post-v1.
 */
export function BlobPreview({ blobId }: BlobPreviewProps) {
  return (
    <section data-artifact-viewer="placeholder">
      <p>
        Blob preview is a slice-10 follow-up. Blob id: <code>{blobId}</code>
      </p>
    </section>
  );
}
