import {
  FormEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { utils } from "ethers";
import KinodeHeader from "../components/KnsHeader";
import { PageProps } from "../lib/types";
import Loader from "../components/Loader";

interface ImportKeyfileProps extends PageProps { }

function ImportKeyfile({
  pw,
  setPw,
  openConnect,
  appSizeOnLoad,
  closeConnect,
  nodeChainId,
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

  // const handlePassword = useCallback(async () => {
  //   try {
  //     const response = await fetch("/vet-keyfile", {
  //       method: "POST",
  //       headers: { "Content-Type": "application/json" },
  //       body: JSON.stringify({
  //         keyfile: localKey,
  //         password: pw,
  //       }),
  //     });

  //     const data = await response.json();

  //     setOsName(data.username);

  //     setPwVet(true);

  //     const errs = [...keyErrs];

  //     const ws = await kns.ws(namehash(data.username));

  //     let index = errs.indexOf(KEY_WRONG_NET_KEY);
  //     if (ws.publicKey !== data.networking_key) {
  //       if (index === -1) errs.push(KEY_WRONG_NET_KEY);
  //     } else if (index !== -1) errs.splice(index, 1);

  //     index = errs.indexOf(KEY_WRONG_IP);
  //     if(ws.ip === 0)
  //       setDirect(false)
  //     else {
  //       setDirect(true)
  //       if (ws.ip !== ipAddress && index === -1)
  //         errs.push(KEY_WRONG_IP);
  //     }

  //     setKeyErrs(errs);
  //   } catch {
  //     setPwVet(false);
  //   }
  //   setPwDebounced(true);
  // }, [localKey, pw, keyErrs, ipAddress, kns, setOsName, setDirect]);

  // const pwDebouncer = useRef<NodeJS.Timeout | null>(null);
  // useEffect(() => {
  //   if (pwDebouncer.current) clearTimeout(pwDebouncer.current);

  //   pwDebouncer.current = setTimeout(async () => {
  //     if (pw !== "") {
  //       if (pw.length < 6)
  //         setPwErr("Password must be at least 6 characters")
  //       else {
  //         setPwErr("")
  //         handlePassword()
  //       }
  //     }
  //   }, 500)

  // }, [pw])

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
          let hashed_password = utils.sha256(utils.toUtf8Bytes(pw));

          const response = await fetch("/vet-keyfile", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              keyfile: localKey,
              password: hashed_password,
            }),
          });

          if (response.status > 399) {
            throw new Error("Incorrect password");
          }

          const result = await fetch("/import-keyfile", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              keyfile: localKey,
              password: hashed_password,
            }),
          });

          if (result.status > 399) {
            throw new Error("Incorrect password");
          }

          const interval = setInterval(async () => {
            const res = await fetch("/");
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
      <KinodeHeader
        header={<h1>Import Keyfile</h1>}
        openConnect={openConnect}
        closeConnect={closeConnect}
        hideConnect
        nodeChainId={nodeChainId}
      />
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
