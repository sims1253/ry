/**
 * @type {import("@vscode/test-cli").TestOptions[]}
 */
module.exports = [
    {
        files: "out/test/**/*.test.js",
        version: "stable",
        mocha: {
            ui: "bdd",
            timeout: 30000,
        },
    },
];
