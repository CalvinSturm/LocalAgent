const test = require("node:test");
const assert = require("node:assert/strict");

const { parseCount } = require("../src/parse");

test("parseCount handles plain input", () => {
  assert.equal(parseCount("7"), 7);
});

test("parseCount trims surrounding whitespace", () => {
  assert.equal(parseCount(" 7 "), 7);
});
