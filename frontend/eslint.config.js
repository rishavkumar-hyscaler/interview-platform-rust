import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import globals from 'globals';
import eslintConfigPrettier from 'eslint-config-prettier';

export default tseslint.config(
  { ignores: ['dist', 'public/proctoring'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/pcm-worklet.js'],
    languageOptions: {
      globals: {
        AudioWorkletProcessor: 'readonly',
        sampleRate: 'readonly',
        registerProcessor: 'readonly',
      },
    },
  },
  {
    languageOptions: { globals: globals.browser },
  },
  eslintConfigPrettier,
);
