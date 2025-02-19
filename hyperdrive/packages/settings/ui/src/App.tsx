import { ConnectButton } from '@rainbow-me/rainbowkit';
import { useAccount } from 'wagmi';
import EditNote from './components/EditNote';
import { useEffect, useState } from 'react';

const APP_PATH = '/settings:settings:sys/ask';

interface Identity {
  name: string;
  networking_key: string;
  ws_routing?: string;
  routers?: string;
}

interface EthRpcSettings {
  public: boolean;
  allow: string[];
  deny: string[];
}

interface ProcessInfo {
  public: boolean;
  on_exit: string;
  wit_version?: string;
  wasm_bytes_handle?: string;
  capabilities: Array<{
    issuer: string;
    params: string;
  }>;
}

interface AppState {
  our_tba: string;
  our_owner: string;
  net_key: string;
  routers: string;
  ip: string;
  tcp_port: string;
  ws_port: string;
  identity: Identity;
  diagnostics: string;
  eth_rpc_providers: any[];
  eth_rpc_access_settings: EthRpcSettings;
  process_map: Record<string, ProcessInfo>;
  stylesheet: string;
}

function App() {
  const [appState, setAppState] = useState<Partial<AppState>>({});
  const [peerPkiResponse, setPeerPkiResponse] = useState('');
  const [peerPingResponse, setPeerPingResponse] = useState('');

  const { address } = useAccount();

  useEffect(() => {
    // Initial data fetch
    fetch(APP_PATH)
      .then(response => response.json())
      .then(data => setAppState(data));

    // WebSocket connection
    const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
    const ws = new WebSocket(wsProtocol + location.host + "/settings:settings:sys/");
    ws.onmessage = event => {
      const data = JSON.parse(event.data);
      setAppState(data);
    };
  }, []);

  const apiCall = async (body: any) => {
    return await fetch(APP_PATH, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  };

  const handleShutdown = () => {
    apiCall("Shutdown");
    setTimeout(() => window.location.reload(), 1000);
  };

  const handleReset = () => {
    apiCall("Reset");
    setTimeout(() => window.location.reload(), 1000);
  };

  const handleSaveStylesheet = () => {
    const stylesheet = (document.getElementById('stylesheet-editor') as HTMLTextAreaElement).value;
    apiCall({ "SetStylesheet": stylesheet });
  };

  const handlePeerPki = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const formData = new FormData(e.currentTarget);
    const response = await fetch(APP_PATH, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ "PeerId": formData.get('peer') }),
    });
    const data = await response.json();
    setPeerPkiResponse(data === null ? "no pki data for peer" : JSON.stringify(data, undefined, 2));
    e.currentTarget.reset();
  };

  const handlePeerPing = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const formData = new FormData(e.currentTarget);
    const form = e.currentTarget;
    const response = await fetch(APP_PATH, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        "Hi": {
          node: formData.get('peer'),
          content: formData.get('content'),
          timeout: Number(formData.get('timeout')),
        }
      }),
    });
    form.reset();
    try {
      const data = await response.json();
      if (data === null) {
        setPeerPingResponse("ping successful!");
      } else if (data === "HiTimeout") {
        setPeerPingResponse("node timed out");
      } else if (data === "HiOffline") {
        setPeerPingResponse("node is offline");
      }
    } catch (err) {
      setPeerPingResponse("ping successful!");
    }
  };

  const handleAddEthProvider = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const formData = new FormData(e.currentTarget);
    const form = e.currentTarget;
    const response = await apiCall({
      "EthConfig": {
        "AddProvider": {
          chain_id: Number(formData.get('chain-id')),
          node_or_rpc_url: { "RpcUrl": formData.get('rpc-url') as string }
        }
      }
    });
    try {
      const data = await response.json();
      console.log(data);
    } catch (err) {
      form.reset();
      // this is actually a success
    }

  };

  const handleRemoveEthProvider = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const formData = new FormData(e.currentTarget);
    const form = e.currentTarget;
    const response = await apiCall({
      "EthConfig": {
        "RemoveProvider": [Number(formData.get('chain-id')), formData.get('rpc-url') as string]
      }
    });
    try {
      const data = await response.json();
      console.log(data);
    } catch (err) {
      form.reset();
      // this is actually a success
    }
  };

  return (
    <div>
      <div id="header">
        <ConnectButton />
      </div>
      <h1>system diagnostics & settings</h1>
      <main>
        <article id="net-diagnostics">
          <h2>networking diagnostics</h2>
          <p id="diagnostics">{appState.diagnostics}</p>
        </article>

        <article id="node-info">
          <h2>node info</h2>
          <p id="node-name">{appState.identity?.name}</p>
          <p id="net-key">{appState.identity?.networking_key}</p>
          {appState.identity?.ws_routing && <p id="ip-ports">{appState.identity.ws_routing}</p>}
          {appState.identity?.routers && <p id="routers">{appState.identity.routers}</p>}
          <div className="mt-16 flex flex-col justify-start">
            <button
              onClick={handleShutdown}
              id="shutdown"
            >
              Shutdown Node
            </button>
            <br />
            <br />
            <button
              onClick={handleReset}
            >
              Reset HNS State
            </button>
          </div>
        </article>

        <article id="pings">
          <h2>fetch PKI data</h2>
          <form id="get-peer-pki" onSubmit={handlePeerPki}>
            <input type="text" name="peer" placeholder="peer-name.os" />
            <button type="submit">get peer info</button>
          </form>
          <p id="peer-pki-response">{peerPkiResponse}</p>
          <h2>ping a node</h2>
          <form id="ping-peer" onSubmit={handlePeerPing}>
            <input type="text" name="peer" placeholder="peer-name.os" />
            <input type="text" name="content" placeholder="message" />
            <input type="number" name="timeout" placeholder="timeout (seconds)" />
            <button type="submit">ping</button>
          </form>
          <p id="peer-ping-response">{peerPingResponse}</p>
        </article>

        <article id="eth-rpc-providers">
          <h2>ETH RPC providers</h2>
          <article id="provider-edits">
            <form id="add-eth-provider" onSubmit={handleAddEthProvider}>
              <input type="number" name="chain-id" placeholder="1" />
              <input type="text" name="rpc-url" placeholder="wss://rpc-url.com" />
              <button type="submit">add provider</button>
            </form>
            <form id="remove-eth-provider" onSubmit={handleRemoveEthProvider}>
              <input type="number" name="chain-id" placeholder="1" />
              <input type="text" name="rpc-url" placeholder="wss://rpc-url.com" />
              <button type="submit">remove provider</button>
            </form>
          </article>
          <ul id="providers">
            {appState.eth_rpc_providers?.map((provider, i) => (
              <li key={i}>{JSON.stringify(provider, undefined, 2)}</li>
            ))}
          </ul>
        </article>

        <article id="eth-rpc-settings">
          <h2>ETH RPC settings</h2>
          <p id="public">status: {appState.eth_rpc_access_settings?.public ? 'public' : 'private'}</p>
          {!appState.eth_rpc_access_settings?.public && (
            <article>
              <p>nodes allowed to connect:</p>
              <ul id="allowed-nodes">
                {appState.eth_rpc_access_settings?.allow.length === 0 ? (
                  <li>(none)</li>
                ) : (
                  appState.eth_rpc_access_settings?.allow.map((node, i) => (
                    <li key={i}>{node}</li>
                  ))
                )}
              </ul>
            </article>
          )}
          <article>
            <p>nodes banned from connecting:</p>
            <ul id="denied-nodes">
              {appState.eth_rpc_access_settings?.deny.length === 0 ? (
                <li>(none)</li>
              ) : (
                appState.eth_rpc_access_settings?.deny.map((node, i) => (
                  <li key={i}>{node}</li>
                ))
              )}
            </ul>
          </article>
        </article>

        <article id="kernel">
          <h2>running processes</h2>
          <ul id="process-map">
            {Object.entries(appState.process_map || {}).map(([id, process]) => (
              <li key={id}>
                <button onClick={(e) => {
                  const details = e.currentTarget.nextElementSibling as HTMLElement;
                  details.style.display = details.style.display === 'none' ? 'block' : 'none';
                }}>{id}</button>
                <div style={{ display: 'none' }}>
                  <p>public: {String(process.public)}</p>
                  <p>on_exit: {process.on_exit}</p>
                  {process.wit_version && <p>wit_version: {process.wit_version}</p>}
                  {process.wasm_bytes_handle && <p>wasm_bytes_handle: {process.wasm_bytes_handle}</p>}
                  <ul>
                    {process.capabilities.map((cap, i) => (
                      <li key={i}>{cap.issuer}({JSON.stringify(JSON.parse(cap.params), null, 2)})</li>
                    ))}
                  </ul>
                </div>
              </li>
            ))}
          </ul>
        </article>

        <article id="id-onchain">
          <h2>identity onchain</h2>
          <p>Only use this utility if you *really* know what you're doing. If edited incorrectly, your node may be unable to connect to the network and require re-registration.</p>
          <br />
          <p>{appState.our_owner && address ? (address.toLowerCase() === appState.our_owner.toLowerCase() ? 'Connected as node owner.' : '**Not connected as node owner. Change wallet to edit node identity.**') : ''}</p>
          <p>TBA: {appState.our_tba}</p>
          <p>Owner: {appState.our_owner}</p>
          <br />
          <p>Routers: {appState.routers || 'none currently, direct node'}</p>
          <EditNote label="~routers" tba={appState.our_tba || ''} field_placeholder="router names, separated by commas (no spaces!)" />
          <p>IP: {appState.ip || 'none currently, indirect node'}</p>
          <EditNote label="~ip" tba={appState.our_tba || ''} field_placeholder="ip address encoded as hex" />
          <p>TCP port: {appState.tcp_port || 'none currently, indirect node'}</p>
          <EditNote label="~tcp-port" tba={appState.our_tba || ''} field_placeholder="tcp port as a decimal number (e.g. 8080)" />
          <p>WS port: {appState.ws_port || 'none currently, indirect node'}</p>
          <EditNote label="~ws-port" tba={appState.our_tba || ''} field_placeholder="ws port as a decimal number (e.g. 8080)" />
          <p>Add a brand new note to your node ID</p>
          <EditNote tba={appState.our_tba || ''} field_placeholder="note content" />
        </article>

        <article id="hyperware-css">
          <h2>stylesheet editor</h2>
          <textarea id="stylesheet-editor" defaultValue={appState.stylesheet} />
          <button id="save-stylesheet" onClick={handleSaveStylesheet}>update hyperware.css</button>
        </article>
      </main>
    </div>
  );
}

export default App;