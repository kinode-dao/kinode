import {
  FormEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { PageProps } from "../lib/types";
import Loader from "../components/Loader";
import { getFetchUrl } from "../utils/fetch";
import { sha256, toBytes } from "viem";

interface ImportKeyfileProps extends PageProps { }

function ImportKeyfile({
  pw,
  setPw,
  appSizeOnLoad,
}: ImportKeyfileProps) {

  const [localKey, setLocalKey] = useState<string>("");
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

  const handleKeyfile = useCallback((e: any) => {
    e.preventDefault();
    const file = e.target.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onloadend = () => {
      setLocalKey(reader.result as string);
      setLocalKeyFileName(file.name);
    };
    reader.readAsText(file);
  }, []);

  const keyfileInputRef = useRef<HTMLInputElement>(null);

  const handleKeyUploadClick = useCallback(async (e: any) => {
    e.preventDefault();
    e.stopPropagation();
    keyfileInputRef.current?.click();
  }, []);

  const handleImportKeyfile = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();

      setLoading(true);

      try {
        if (keyErrs.length === 0 && localKey !== "") {
          let hashed_password = sha256(toBytes(pw));

          const result = await fetch(getFetchUrl("/import-keyfile"), {
            method: "POST",
            credentials: 'include',
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              keyfile: localKey,
              password_hash: hashed_password,
            }),
          });

          if (result.status > 399) {
            throw new Error("Incorrect password");
          }

          const interval = setInterval(async () => {
            const res = await fetch(getFetchUrl("/"), { credentials: 'include' });
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
    <>
      {loading ? (
        <Loader msg="Setting up node..." />
      ) : (
        <form id="signup-form" className="flex flex-col max-w-[450px]" onSubmit={handleImportKeyfile}>
          <div
            className="flex flex-col self-start place-content-center w-full"
          >
            <h4 className="my-2 flex">
              {" "}
              1. Upload Keyfile{" "}
            </h4>
            {Boolean(localKeyFileName) && (
              <h5 className="underline mb-2">
                {" "}
                {localKeyFileName ? localKeyFileName : ".keyfile"}{" "}
              </h5>
            )}
            <button type="button" onClick={handleKeyUploadClick}>
              {localKeyFileName ? "Change" : "Select"} Keyfile
            </button>
            <input
              ref={keyfileInputRef}
              className="hidden"
              type="file"
              onChange={handleKeyfile}
            />
          </div>

          <div className="flex flex-col w-full">
            <h4 className="my-2 flex">
              {" "}
              2. Enter Password{" "}
            </h4>

            <input
              type="password"
              id="password"
              required
              minLength={6}
              name="password"
              placeholder="Min 6 characters"
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              className="mb-2"
            />

            {pwErr && (
              <div className="flex">
                {" "}
                <p className="text-red-500"> {pwErr} </p>{" "}
              </div>
            )}
            {pwDebounced && !pwVet && 6 <= pw.length && (
              <div className="flex">
                {" "}
                <p className="text-red-500"> Password is incorrect </p>{" "}
              </div>
            )}
          </div>

          <div className="flex flex-col w-full mb-2">
            {keyErrs.map((x, i) => (
              <span key={i} className="key-err">
                {x}
              </span>
            ))}
            <button type="submit"> Import Keyfile </button>
          </div>
          <p className="text-sm">
            Please note: if the original node was booted as a direct node
            (static IP), then you must run this node from the same IP. If not,
            you will have networking issues. If you need to change the network
            options, please go back and select "Reset OsName".
          </p>
        </form>
      )}
    </>
  );
}

export default ImportKeyfile;
