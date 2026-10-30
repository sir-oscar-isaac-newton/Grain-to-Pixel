import js from '@eslint/js';
import globals from 'globals';
import jsxA11y from 'eslint-plugin-jsx-a11y';
import react from 'eslint-plugin-react';
import tseslint from 'typescript-eslint';

// Convention section 1: strict TypeScript, no any, no console, accessible JSX,
// and no literal strings in JSX (all user-visible text goes through t()).
export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/target/**',
      'packages/ipc-types/src/bindings.ts',
      'tools/scripts/check-deps/tests/fixtures/**',
    ],
  },
  js.configs.recommended,
  {
    files: ['**/*.{js,mjs}'],
    languageOptions: { globals: globals.node },
  },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [...tseslint.configs.strict],
    languageOptions: { globals: { ...globals.browser } },
    plugins: { 'jsx-a11y': jsxA11y, react },
    settings: { react: { version: 'detect' } },
    rules: {
      ...jsxA11y.flatConfigs.recommended.rules,
      'no-console': 'error',
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/ban-ts-comment': [
        'error',
        { 'ts-expect-error': 'allow-with-description', 'ts-ignore': true },
      ],
      'react/jsx-no-literals': ['error', { noStrings: true, ignoreProps: true }],
    },
  },
);
