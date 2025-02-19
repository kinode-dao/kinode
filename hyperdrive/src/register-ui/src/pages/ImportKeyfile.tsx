import {
  FormEvent,
  useCallback,
  useEffect,
  useState,
} from "react";
import { PageProps } from "../lib/types";
import Loader from "../components/Loader";
import { redirectToHomepage } from "../utils/redirect-to-homepage";

interface ImportKeyfileProps extends PageProps { }

function ImportKeyfile({
  pw,
  setPw,
}: ImportKeyfileProps) {

  const [localKey, setLocalKey] = useState<Uint8Array | null>(null);
  const [localKeyFileName, setLocalKeyFileName] = useState<string>("");
  const [keyErrs, _setKeyErrs] = useState<string[]>([]);

  const [pwErr, _setPwErr] = useState<string>("");
  const [pwVet, _setPwVet] = useState<boolean>(false);
  const [pwDebounced, _setPwDebounced] = useState<boolean>(false);
  const [loading, setLoading] = useState<boolean>(false);
  const [hnsName, setHnsName] = useState<string>("");

  useEffect(() => {
    document.title = "Import Keyfile";
  }, []);

  // for if we check router validity in future
  // const KEY_BAD_ROUTERS = "Routers from records are offline"

  const handleKeyfile = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    e.preventDefault();
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onloadend = () => {
      if (reader.result instanceof ArrayBuffer) {
        setLocalKey(new Uint8Array(reader.result));
        setLocalKeyFileName(file.name);
      }
    };
    reader.readAsArrayBuffer(file);
  }, []);

  const handleImportKeyfile = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();

      setLoading(true);

      try {
        if (keyErrs.length === 0 && localKey !== null) {
          argon2.hash({ pass: pw, salt: hnsName, hashLen: 32, time: 2, mem: 19456, type: argon2.ArgonType.Argon2id }).then(async h => {
            const hashed_password_hex = `0x${h.hashHex}`;

            const result = await fetch("/import-keyfile", {
              method: "POST",
              credentials: 'include',
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({
                keyfile: Buffer.from(localKey).toString('utf8'),
                password_hash: hashed_password_hex,
              }),
            });

            if (result.status > 399) {
              throw new Error("Incorrect password");
            }
            redirectToHomepage();
          }).catch(err => {
            window.alert(String(err));
            setLoading(false);
          });
        }
      } catch (err) {
        window.alert(String(err));
        setLoading(false);
      }
    },
    [localKey, pw, keyErrs]
  );

  return (
    <div className="container fade-in">
      <button onClick={() => history.back()} className="button secondary back">🔙</button>
      <div className="section">
        {loading ? (
          <Loader msg="Setting up node..." />
        ) : (
          <>
            <form className="form" onSubmit={handleImportKeyfile}>
              <div className="form-group">
                <h4 className="form-label">1. Upload Keyfile</h4>
                <label className="file-input-label">
                  <input
                    type="file"
                    className="file-input"
                    onChange={handleKeyfile}
                  />
                  <span className="button secondary">
                    {localKeyFileName ? "Change Keyfile" : "Select Keyfile"}
                  </span>
                </label>
                {localKeyFileName && <p className="mt-2">{localKeyFileName}</p>}
              </div>
              <div className="form-group">
                <h4 className="form-label">2. Enter Node ID</h4>
                <label className="name-input-label">
                  <input
                    type="text"
                    className="name-input"
                    onChange={(e) => setHnsName(e.target.value)}
                  />
                </label>
              </div>
              <div className="form-group">
                <h4 className="form-label">3. Enter Password</h4>
                <input
                  type="password"
                  id="password"
                  required
                  minLength={6}
                  name="password"
                  placeholder=""
                  value={pw}
                  onChange={(e) => setPw(e.target.value)}
                />
                {pwErr && <p className="error-message">{pwErr}</p>}
                {pwDebounced && !pwVet && 6 <= pw.length && (
                  <p className="error-message">Password is incorrect!</p>
                )}
              </div>

              <div className="form-group">
                {keyErrs.map((x, i) => (
                  <p key={i} className="error-message">{x}</p>
                ))}
                <button type="submit" className="button">Boot Node</button>
              </div>
              <p className="text-sm mt-2">
                Please note: if the original node was booted as a direct node
                (static IP), then you must run this node from the same IP. If not,
                you will have networking issues. If you need to change the network
                options, please go back and select "Reset OsName".
              </p>
            </form>
          </>
        )}
      </div>
    </div>
  );
}

export default ImportKeyfile;
