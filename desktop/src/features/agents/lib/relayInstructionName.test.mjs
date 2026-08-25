import assert from "node:assert/strict";
import test from "node:test";

import { relayInstructionNameFromTitle } from "./relayInstructionName.ts";

test("keeps ordinary titles readable", () => {
  assert.equal(
    relayInstructionNameFromTitle("Engineering discipline"),
    "engineering-discipline",
  );
  assert.equal(relayInstructionNameFromTitle("Déjà vu"), "deja-vu");
});

test("disambiguates meaningful punctuation", () => {
  const plain = relayInstructionNameFromTitle("C");
  const cpp = relayInstructionNameFromTitle("C++");
  const csharp = relayInstructionNameFromTitle("C#");

  assert.equal(plain, "c");
  assert.match(cpp, /^c-[a-z0-9]{7}$/);
  assert.match(csharp, /^c-[a-z0-9]{7}$/);
  assert.notEqual(cpp, csharp);
});

test("creates a deterministic fallback for non-Latin titles", () => {
  assert.equal(
    relayInstructionNameFromTitle("設計レビュー"),
    "instruction-1xvscj8",
  );
});

test("keeps long names bounded and disambiguated", () => {
  const sharedPrefix = "a".repeat(80);
  const first = relayInstructionNameFromTitle(`${sharedPrefix} one`);
  const second = relayInstructionNameFromTitle(`${sharedPrefix} two`);

  assert.equal(first.length, 64);
  assert.equal(second.length, 64);
  assert.notEqual(first, second);
});
