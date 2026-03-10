// @ts-check

/**
 * @param {string} input
 * @returns {number}
 */
function parseCount(input) {
  if (!/^\d+$/.test(input)) {
    return NaN;
  }
  return Number.parseInt(input, 10);
}

module.exports = { parseCount };
