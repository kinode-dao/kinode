import {
  FormEvent,
  useCallback,
  useEffect,
  useState,
} from "react";
import { PageProps } from "../lib/types";
import Loader from "../components/Loader";
import { sha256, toBytes } from "viem";

interface ImportKeyfileProps extends PageProps { }

function ImportKeyfile({
  pw,
  setPw,
  appSizeOnLoad,
}: ImportKeyfileProps) {

  const [localKey, setLocalKey] = useState<Uint8Array | null>(null);
  const [localKeyFileName, setLocalKeyFileName] = useState<string>("");
  const [keyErrs, _setKeyErrs] = useState<string[]>([]);

  const [pwErr, _setPwErr] = useState<string>("");
  const [pwVet, _setPwVet] = useState<boolean>(false);
  const [pwDebounced, _setPwDebounced] = useState<boolean>(false);
  const [loading, setLoading] = useState<boolean>(false);

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
          let hashed_password = sha256(toBytes(pw));

          const result = await fetch("/import-keyfile", {
            method: "POST",
            credentials: 'include',
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              keyfile: Buffer.from(localKey).toString('base64'),
              password_hash: hashed_password,
            }),
          });

          if (result.status > 399) {
            throw new Error("Incorrect password");
          }

          const interval = setInterval(async () => {
            const res = await fetch("/", { credentials: 'include' });
            if (
              res.status < 300 &&
              Number(res.headers.get("content-length")) !== appSizeOnLoad
            ) {
              clearInterval(interval);
              window.location.replace("/");
            }
          }, 2000);
        }
      } catch {
        window.alert("An error occurred, please try again.");
        setLoading(false);
      }
    },
    [localKey, pw, keyErrs, appSizeOnLoad]
  );

  return (
    <div className="container fade-in">
      <div className="section">
        {loading ? (
          <Loader msg="Setting up node..." />
        ) : (
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
            </div>            <div className="form-group">
              <h4 className="form-label">2. Enter Password</h4>
              <input
                type="password"
                id="password"
                required
                minLength={6}
                name="password"
                placeholder="Min 6 characters"
                value={pw}
                onChange={(e) => setPw(e.target.value)}
              />
              {pwErr && <p className="error-message">{pwErr}</p>}
              {pwDebounced && !pwVet && 6 <= pw.length && (
                <p className="error-message">Password is incorrect</p>
              )}
            </div>

            <div className="form-group">
              {keyErrs.map((x, i) => (
                <p key={i} className="error-message">{x}</p>
              ))}
              <button type="submit" className="button">Import Keyfile</button>
            </div>
            <p className="text-sm mt-2">
              Please note: if the original node was booted as a direct node
              (static IP), then you must run this node from the same IP. If not,
              you will have networking issues. If you need to change the network
              options, please go back and select "Reset OsName".
            </p>
          </form>
        )}
      </div>
    </div>
  );
}

export default ImportKeyfile;
