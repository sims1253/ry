/**
 * @type {import("@vscode/test-cli").TestOptions}
 */
module.exports = {
    files: "out/test/**/*.test.js",
    vscodeLauncher: {
        version: "stable",
    },
};
