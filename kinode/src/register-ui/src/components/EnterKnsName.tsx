import React, { useEffect, useRef, useState } from "react";
import isValidDomain from "is-valid-domain";
import { toAscii } from "idna-uts46-hx";
import { usePublicClient } from 'wagmi'

import { KIMAP, kimapAbi } from '../abis'
import { kinohash } from "../utils/kinohash";

export const NAME_URL = "Name must contain only valid characters (a-z, 0-9, and -)";
export const NAME_LENGTH = "Name must be 9 characters or more";
export const NAME_CLAIMED = "Name is already claimed";
export const NAME_INVALID_PUNY = "Unsupported punycode character";
export const NAME_NOT_OWNER = "Name already exists and does not belong to this wallet";
export const NAME_NOT_REGISTERED = "Name is not registered";

type EnterNameProps = {
  address?: `0x${string}`;
  name: string;
  fixedTlz?: string;
  setName: React.Dispatch<React.SetStateAction<string>>;
  nameValidities: string[];
  setNameValidities: React.Dispatch<React.SetStateAction<string[]>>;
  triggerNameCheck: boolean;
  setTba?: React.Dispatch<React.SetStateAction<string>>;
  isReset?: boolean;
};

function EnterKnsName({
  address,
  name,
  setName,
  fixedTlz,
  nameValidities,
  setNameValidities,
  triggerNameCheck,
  setTba,
  isReset = false,
}: EnterNameProps) {
  const client = usePublicClient();
  const debouncer = useRef<NodeJS.Timeout | null>(null);

  const [isPunyfied, setIsPunyfied] = useState('');

  useEffect(() => {
    if (debouncer.current) clearTimeout(debouncer.current);

    debouncer.current = setTimeout(async () => {
      let index: number;
      let validities: string[] = [];
      setIsPunyfied('');

      if (/[A-Z]/.test(name)) {
        validities.push(NAME_URL);
        setNameValidities(validities);
        return;
      }

      let normalized = ''
      index = validities.indexOf(NAME_INVALID_PUNY);
      try {
        normalized = toAscii(fixedTlz ? name + fixedTlz : name);
        if (index !== -1) validities.splice(index, 1);
      } catch (e) {
        if (index === -1) validities.push(NAME_INVALID_PUNY);
      }

      if (fixedTlz === '.os') {
        const len = [...normalized].length - 3;
        index = validities.indexOf(NAME_LENGTH);
        if (len < 9 && len !== 0) {
          if (index === -1) validities.push(NAME_LENGTH);
        } else if (index !== -1) validities.splice(index, 1);
      }

      if (normalized !== (fixedTlz ? name + fixedTlz : name)) setIsPunyfied(normalized);

      // only check if name is valid punycode
      if (normalized) {
        index = validities.indexOf(NAME_URL);
        if (name !== "" && !isValidDomain(normalized)) {
          if (index === -1) validities.push(NAME_URL);
        } else if (index !== -1) {
          validities.splice(index, 1);
        }

        index = validities.indexOf(NAME_CLAIMED);

        // only check if name is valid and long enough
        if (validities.length === 0 || index !== -1 && normalized.length > 2) {
          try {
            const namehash = kinohash(normalized)
            // maybe separate into helper function for readability?
            // also note picking the right chain ID & address!
            const data = await client?.readContract({
              address: KIMAP,
              abi: kimapAbi,
              functionName: "get",
              args: [namehash]
            })

            const tba = data?.[0];
            if (tba !== undefined) {
              setTba ? (setTba(tba)) : null;
            } else {
              validities.push(NAME_NOT_REGISTERED);
            }

            const owner = data?.[1];
            const owner_is_zero = owner === "0x0000000000000000000000000000000000000000";

            if (!owner_is_zero && !isReset) validities.push(NAME_CLAIMED);

            if (!owner_is_zero && isReset && address && owner !== address) validities.push(NAME_NOT_OWNER);

            if (isReset && owner_is_zero) validities.push(NAME_NOT_REGISTERED);
          } catch (e) {
            console.error({ e })
            if (index !== -1) validities.splice(index, 1);
          }
        }
      }
      setNameValidities(validities);
    }, 500);
  }, [name, triggerNameCheck, isReset]);

  const noSpaces = (e: any) =>
    e.target.value.indexOf(" ") === -1 && setName(e.target.value);

  return (
    <div className="enter-kns-name">
      <div className="input-wrapper">
        <input
          value={name}
          onChange={noSpaces}
          type="text"
          required
          name="kns-name"
          placeholder="mynode123"
          className="kns-input"
        />
        {fixedTlz && <span className="kns-suffix">{fixedTlz}</span>}
      </div>
      {nameValidities.map((x, i) => (
        <p key={i} className="error-message">{x}</p>
      ))}
      {isPunyfied !== '' && <p className="puny-warning">special characters will be converted to punycode: {isPunyfied}</p>}
    </div>
  );
}

export default EnterKnsName;
