import { ConnectButton } from '@rainbow-me/rainbowkit';

function App() {
  return (
    <div>
      <div id="header">
        <ConnectButton />
      </div>
      <h1>system diagnostics & settings</h1>
      <main>
        <article id="net-diagnostics">
          <h2>networking diagnostics</h2>
          <p id="diagnostics"></p>
        </article>

        <article id="node-info">
          <h2>node info</h2>
          <p id="node-name"></p>
          <p id="net-key"></p>
          <p id="ip-ports"></p>
          <p id="routers"></p>
          <button id="shutdown">shut down node(!)</button>
        </article>

        <article id="pings">
          <h2>fetch PKI data</h2>
          <form id="get-peer-pki">
            <input type="text" name="peer" placeholder="peer-name.os" />
            <button type="submit">get peer info</button>
          </form>
          <p id="peer-pki-response"></p>
          <h2>ping a node</h2>
          <form id="ping-peer">
            <input type="text" name="peer" placeholder="peer-name.os" />
            <input type="text" name="content" placeholder="message" />
            <input type="number" name="timeout" placeholder="timeout (seconds)" />
            <button type="submit">ping</button>
          </form>
          <p id="peer-ping-response"></p>
        </article>

        <article id="eth-rpc-providers">
          <h2>ETH RPC providers</h2>
          <article id="provider-edits">
            <form id="add-eth-provider">
              <input type="number" name="chain-id" placeholder="1" />
              <input type="text" name="rpc-url" placeholder="wss://rpc-url.com" />
              <button type="submit">add provider</button>
            </form>
            <form id="remove-eth-provider">
              <input type="number" name="chain-id" placeholder="1" />
              <input type="text" name="rpc-url" placeholder="wss://rpc-url.com" />
              <button type="submit">remove provider</button>
            </form>
          </article>
          <ul id="providers"></ul>
        </article>

        <article id="eth-rpc-settings">
          <h2>ETH RPC settings</h2>
          <p id="public"></p>
          <article>
            <p>nodes allowed to connect:</p>
            <ul id="allowed-nodes"></ul>
          </article>
          <article>
            <p>nodes banned from connecting:</p>
            <ul id="denied-nodes"></ul>
          </article>
        </article>

        <article id="kernel">
          <h2>running processes</h2>
          <ul id="process-map"></ul>
        </article>

        <article id="kinode-css">
          <h2>stylesheet editor</h2>
          <textarea id="stylesheet-editor"></textarea>
          <button id="save-stylesheet">update kinode.css</button>
        </article>
      </main>
    </div>
  );
}

export default App;