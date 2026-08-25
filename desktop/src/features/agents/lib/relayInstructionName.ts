const MAX_RELAY_INSTRUCTION_NAME_LENGTH = 64;

/** Derives a stable relay-safe identifier without exposing an extra form field. */
export function relayInstructionNameFromTitle(title: string): string {
  const searchableTitle = title
    .trim()
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .trim();
  const readableName = searchableTitle
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/-{2,}/g, "-");
  const canonicalTitle = searchableTitle.replace(/[\s-]+/g, " ");
  const canonicalName = readableName.replace(/-+/g, " ");

  if (
    readableName &&
    readableName.length <= MAX_RELAY_INSTRUCTION_NAME_LENGTH &&
    canonicalTitle === canonicalName
  ) {
    return readableName;
  }

  const hash = shortTitleHash(title.trim());
  const prefix = (readableName || "instruction")
    .slice(0, MAX_RELAY_INSTRUCTION_NAME_LENGTH - hash.length - 1)
    .replace(/-+$/g, "");
  return `${prefix}-${hash}`;
}

function shortTitleHash(title: string): string {
  let hash = 2_166_136_261;
  for (const character of title) {
    hash ^= character.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16_777_619);
  }
  return (hash >>> 0).toString(36).padStart(7, "0");
}
