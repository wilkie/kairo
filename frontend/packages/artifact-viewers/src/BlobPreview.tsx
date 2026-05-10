// Composite component: fetches a blob's bytes, sniffs them,
// picks a viewer from the registry, and renders the result.
//
// The fetch goes through TanStack Query so the cache + loading
// state behave like every other inspector hook. We materialize
// the response as `Uint8Array` (not the raw `Blob` the api-
// client returns) so the cache holds plain bytes and the
// sniffer / viewers don't each re-decode the response.
//
// Inspector blobs are typically small (manifests, configs) so
// caching in memory is fine. If a future viewer needs to
// stream a multi-GB pack, it should reach for the raw blob
// imperatively instead of going through this component.

import { useQuery } from '@tanstack/react-query';
import { kairoKeys, useKairoClient, type KairoApiClientError } from '@kairo/api-client';
import { ErrorDisplay } from '@kairo/ui';
import Skeleton from '@mui/material/Skeleton';
import Stack from '@mui/material/Stack';
import { useMemo } from 'react';
import {
  defaultArtifactViewers,
  pickViewer,
} from './registry';
import { sniffArtifact } from './sniffer';
import type { ArtifactRecord, ArtifactViewer } from './types';

export interface BlobPreviewProps {
  blobId: string;
  /** Override the viewer registry. Defaults to
   * {@link defaultArtifactViewers}. */
  registry?: ReadonlyArray<ArtifactViewer>;
}

export function BlobPreview({ blobId, registry = defaultArtifactViewers }: BlobPreviewProps) {
  const client = useKairoClient();
  const blobQ = useQuery<Uint8Array, KairoApiClientError>({
    queryKey: kairoKeys.blob(blobId),
    queryFn: async () => {
      const blob = await client.getBlob(blobId);
      const buffer = await blob.arrayBuffer();
      return new Uint8Array(buffer);
    },
  });

  const record: ArtifactRecord | null = useMemo(() => {
    if (blobQ.data === undefined) return null;
    return {
      blobId,
      bytes: blobQ.data,
      sniff: sniffArtifact(blobQ.data),
    };
  }, [blobId, blobQ.data]);

  if (blobQ.isPending) {
    return (
      <Stack spacing={1}>
        <Skeleton variant="text" width="60%" />
        <Skeleton variant="rectangular" height={120} />
      </Stack>
    );
  }
  if (blobQ.isError) {
    return (
      <ErrorDisplay
        title="Could not load blob"
        message="The blob endpoint returned an error."
        detail={renderDetail(blobQ.error)}
      />
    );
  }
  if (record === null) {
    return null;
  }

  const viewer = pickViewer(record, registry);
  if (viewer === null) {
    return (
      <ErrorDisplay
        title="No viewer available"
        message="This artifact's bytes did not match any registered viewer."
      />
    );
  }

  const Viewer = viewer.Viewer;
  return <Viewer record={record} />;
}

function renderDetail(error: KairoApiClientError | null): string | undefined {
  if (error === null) return undefined;
  const d = error.detail;
  switch (d.kind) {
    case 'network':
      return `network: ${d.message}`;
    case 'daemon':
      return `daemon: ${d.code}${d.status === undefined ? '' : ` (HTTP ${d.status})`}: ${d.message}`;
    case 'decode':
      return `decode: ${d.message}`;
  }
}

/** Read the chosen viewer's id/label without rendering — useful
 * for parent components that want to show "Viewing as JSON"
 * affordances next to the panel. Returns null when the bytes
 * haven't loaded yet. */
export function useChosenArtifactViewer(
  blobId: string,
  registry: ReadonlyArray<ArtifactViewer> = defaultArtifactViewers,
): ArtifactViewer | null {
  const client = useKairoClient();
  const blobQ = useQuery<Uint8Array, KairoApiClientError>({
    queryKey: kairoKeys.blob(blobId),
    queryFn: async () => {
      const blob = await client.getBlob(blobId);
      const buffer = await blob.arrayBuffer();
      return new Uint8Array(buffer);
    },
  });
  return useMemo(() => {
    if (blobQ.data === undefined) return null;
    const record: ArtifactRecord = {
      blobId,
      bytes: blobQ.data,
      sniff: sniffArtifact(blobQ.data),
    };
    return pickViewer(record, registry);
  }, [blobId, blobQ.data, registry]);
}
