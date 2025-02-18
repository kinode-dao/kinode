# App Store UI

This UI template uses the React framework compiled with Vite.
It is based on the Vite React Typescript template.

## Setup

There are 2 ways to set up this repo:
1. Place this repo next to the `pkg` repo of your Hyperware project (usually the top level).
2. Set `BASE_URL` in `vite.config.ts` to the URL of your Hyperware project (i.e. `/chess:chess:sys`) and comment out lines 8 and 9.

## Development

Run `npm i`, `npm run tc`, and then `npm start` to start working on the UI.
By default, the dev server will proxy requests to `http://localhost:8080`.
You can change the proxy target by setting `VITE_NODE_URL` like so:
`VITE_NODE_URL=http://localhost:8081 npm start`

You may see an error:

```
[vite] Pre-transform error: Failed to load url /our.js (resolved id: /our.js). Does the file exist?
```

You can safely ignore this error. The file will be served by the node via the proxy.

#### public vs assets

The `public/assets` folder contains files that are referenced in `index.html`, `src/assets` is for asset files that are only referenced in `src` code.

## Building

Run `npm run build`, the build will be generated in the `dist` directory.
If this repo is next to your Hyperware `pkg` directory then you can `npm run build:copy` to build and copy it for installation.

## About Vite + React

This template provides a minimal setup to get React working in Vite with HMR and some ESLint rules.

Currently, two official plugins are available:

- [@vitejs/plugin-react](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react/README.md) uses [Babel](https://babeljs.io/) for Fast Refresh
- [@vitejs/plugin-react-swc](https://github.com/vitejs/vite-plugin-react-swc) uses [SWC](https://swc.rs/) for Fast Refresh

## Expanding the ESLint configuration

If you are developing a production application, we recommend updating the configuration to enable type aware lint rules:

- Configure the top-level `parserOptions` property like this:

```js
export default {
  // other rules...
  parserOptions: {
    ecmaVersion: 'latest',
    sourceType: 'module',
    project: ['./tsconfig.json', './tsconfig.node.json'],
    tsconfigRootDir: __dirname,
  },
}
```

- Replace `plugin:@typescript-eslint/recommended` to `plugin:@typescript-eslint/recommended-type-checked` or `plugin:@typescript-eslint/strict-type-checked`
- Optionally add `plugin:@typescript-eslint/stylistic-type-checked`
- Install [eslint-plugin-react](https://github.com/jsx-eslint/eslint-plugin-react) and add `plugin:react/recommended` & `plugin:react/jsx-runtime` to the `extends` list
