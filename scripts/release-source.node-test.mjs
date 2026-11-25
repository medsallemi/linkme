import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { releaseRepository } from "./release-source.mjs";

const { repository } = JSON.parse(
  await readFile(new URL("../package.json", import.meta.url), "utf8"),
);

test("production packaging uses the approved repository metadata", () => {
  assert.equal(releaseRepository(repository.url), "medsallemi/linkme");
});

test("testing can use another repository or turn off checks", () => {
  assert.equal(
    releaseRepository(repository.url, "fixtures/update-test"),
    "fixtures/update-test",
  );
  assert.equal(releaseRepository(repository.url, ""), null);
});

test("rejects malformed sources without echoing their values", () => {
  for (const value of [
    "https://github.com/fixtures/update-test",
    "fixtures/..",
    "fixtures/update-test.git",
    "fixtures/update-test?key=synthetic",
    "fixtures/update-test/extra",
    "-fixtures/update-test",
  ]) {
    assert.throws(() => releaseRepository(repository.url, value), {
      message: "Invalid GitHub release repository. Use OWNER/REPO.",
    });
  }
  assert.throws(() =>
    releaseRepository("https://example.org/fixtures/update-test.git"),
  );
});
