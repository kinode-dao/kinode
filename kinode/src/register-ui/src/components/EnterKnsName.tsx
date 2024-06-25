import React, { useEffect, useRef } from "react";
import isValidDomain from "is-valid-domain";
import hash from "@ensdomains/eth-ens-namehash";
import { toAscii } from "idna-uts46-hx";

type ClaimOsNameProps = {
  name: string;
  setName: React.Dispatch<React.SetStateAction<string>>;
  nameValidities: string[];
  setNameValidities: React.Dispatch<React.SetStateAction<string[]>>;
  triggerNameCheck: boolean;
  isReset?: boolean;
};

function EnterKnsName({
  name,
  setName,
  nameValidities,
  setNameValidities,
  triggerNameCheck,
  isReset = false,
}: ClaimOsNameProps) {
  const NAME_URL =
    "Name must be a valid URL without subdomains (A-Z, a-z, 0-9, and punycode)";
  const NAME_LENGTH = "Name must be 9 characters or more";
  const NAME_CLAIMED = "Name is already claimed";
  const NAME_INVALID_PUNY = "Unsupported punycode character";

  const debouncer = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    if (isReset) return;

    if (debouncer.current) clearTimeout(debouncer.current);

    debouncer.current = setTimeout(async () => {
      let index: number;
      let validities = [...nameValidities];

      const len = [...name].length;
      index = validities.indexOf(NAME_LENGTH);
      if (len < 9 && len !== 0) {
        if (index === -1) validities.push(NAME_LENGTH);
      } else if (index !== -1) validities.splice(index, 1);

      let normalized = ''
      index = validities.indexOf(NAME_INVALID_PUNY);
      try {
        normalized = toAscii(name + ".os");
        if (index !== -1) validities.splice(index, 1);
      } catch (e) {
        if (index === -1) validities.push(NAME_INVALID_PUNY);
      }

      // only check if name is valid punycode
      if (normalized && normalized !== '.os') {
        index = validities.indexOf(NAME_URL);
        if (name !== "" && !isValidDomain(normalized)) {
          if (index === -1) validities.push(NAME_URL);
        } else if (index !== -1) validities.splice(index, 1);

        index = validities.indexOf(NAME_CLAIMED);

        if (validities.length === 0 || index !== -1 && normalized.length > 2) {
          try {
            const namehash = hash.hash(normalized)
            // TODO
            const owner = "dotOs.ownerOf(namehash)" // await dotOs?.ownerOf(namehash);
            if (owner && index === -1) validities.push(NAME_CLAIMED);
          } catch (e) {
            console.error({ e })
            if (index !== -1) validities.splice(index, 1);
          }
        }
      }

      setNameValidities(validities);
    }, 500);
  }, [name, triggerNameCheck, isReset]);

  const noDots = (e: any) =>
    e.target.value.indexOf(".") === -1 && setName(e.target.value);

  return (
    <div className="flex flex-col w-full place-items-center place-content-center">
      <div className="flex w-full place-items-center">
        <input
          value={name}
          onChange={noDots}
          type="text"
          required
          name="dot-os-name"
          placeholder="e.g. myname"
          className="grow"
        />
        <div className="ml-2 text-lg">.os</div>
      </div>
      {nameValidities.map((x, i) => (
        <div key={i}>
          <br />
          <span className="text-red-500">{x}</span>
        </div>
      ))}
    </div>
  );
}

export default EnterKnsName;
