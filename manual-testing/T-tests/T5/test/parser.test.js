const test = require("node:test");
const assert = require("node:assert/strict");

const { parseCount } = require("../src");

test("parseCount handles plain input", () => {
  assert.equal(parseCount("7"), 7);
});

test("parseCount trims whitespace before parsing", () => {
  assert.equal(parseCount(" 7 \n"), 7);
});
