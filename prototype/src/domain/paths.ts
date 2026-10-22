// Greg's rule: paths never contain spaces. Words join with hyphens; a date
// prefix joins with an underscore, e.g. ~/Pictures/Argus/2026-10-12_tahoe.

function slug(name: string) {
  return name
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[^\w\s-]/g, ' ')
    .trim()
    .replace(/[\s_-]+/g, '-');
}

/** Clean a path the user typed: spaces become hyphens. */
export function noSpaces(path: string) {
  return path.replace(/\s+/g, '-');
}

export function shootFolder(date: string, name: string) {
  return `~/Pictures/Argus/${date}_${slug(name.replace(/,?\s*\w{3}\s+\d{1,2}$/, '')) || 'shoot'}`;
}
