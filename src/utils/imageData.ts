// Pasted images (notes, terminal): which types we keep and how they travel
// to the backend.

export const IMAGE_EXTS: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
}

export function imageFiles(list: DataTransferItemList | undefined | null): File[] {
  if (!list) return []
  const files: File[] = []
  for (const item of Array.from(list)) {
    if (item.kind !== 'file' || !IMAGE_EXTS[item.type]) continue
    const file = item.getAsFile()
    if (file) files.push(file)
  }
  return files
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer())
  // Chunked so large screenshots don't overflow the argument limit
  let binary = ''
  const CHUNK = 0x8000
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK))
  }
  return btoa(binary)
}
