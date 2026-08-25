import type { RelaySkillCover } from "@/shared/api/tauriPersonas";

export function toggleRelaySkillCoordinate(
  selected: readonly string[],
  skill: RelaySkillCover,
): string[] {
  if (!skill.compatible) return [...selected];

  return selected.includes(skill.coordinate)
    ? selected.filter((coordinate) => coordinate !== skill.coordinate)
    : [...selected, skill.coordinate];
}
