# Register
This app is compiled and put into the root directory of every Kinode node for login and registration. It handles all on-chain KNS registration flows

## Development

1. Run `yarn` to install dependencies
2. Run `yarn run tc` to generate ABIs
3. Start a kinode locally on port 8080 (default)
3. Run `yarn start` to serve the UI at http://localhost:3000 (proxies requests to local kinode)

If you would like to proxy requests to a kinode that is not at http://localhost:8080, change the `proxy` field in `package.json`.

## Building

1. Run `yarn` to install dependencies
2. Run `yarn run tc` to generate ABIs
3. Run `yarn build` to generate the `./build` folder
4. Overwrite `kinode/kinode/src/register-ui/build` with `./build`
