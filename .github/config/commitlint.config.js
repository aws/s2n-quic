module.exports = {
  extends: ['@commitlint/config-conventional'],
   // refer to https://commitlint.js.org/reference/rules-configuration.html#rules-configuration for rule configurations
   rules: {
    'subject-case': [0],
    'body-max-line-length': [0],
    'footer-max-line-length': [0],
    'type-enum': [2, 'always', [
      'build', 'chore', 'ci', 'docs', 'feat', 'fix', 'perf', 'refactor', 'revert', 'style', 'test',
      'opt', 'internal', 'tests'
    ]]
  },
  parserPreset: {
    parserOpts: {
      issuePrefixes: ['#']
    }
  },
  ignores: [
    (message) => message.includes('Co-authored-by:')
  ]
};
