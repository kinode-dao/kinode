import React, { useEffect, useRef, useState } from "react";
import { toAscii } from "idna-uts46-hx";
import { usePublicClient } from 'wagmi'

import { HYPERMAP, hypermapAbi } from '../abis'
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

function EnterHnsName({
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
      let validities: string[] = [];
      setIsPunyfied('');

      if (fixedTlz) {
        if (!/^[a-z0-9-]*$/.test(name)) {
          validities.push(NAME_URL);
          setNameValidities(validities);
          return;
        }
      } else {
        if (!/^[a-z0-9-.]*$/.test(name)) {
          validities.push(NAME_URL);
          setNameValidities(validities);
          return;
        }
      }

      let normalized = ''
      try {
        normalized = toAscii(fixedTlz ? name + fixedTlz : name);
      } catch (e) {
        validities.push(NAME_INVALID_PUNY);
      }

      // length check, only for .os
      if (fixedTlz === '.os') {
        const len = [...normalized].length - 3;
        if (len < 9 && len !== 0) {
          validities.push(NAME_LENGTH);
        }
      }

      if (normalized !== (fixedTlz ? name + fixedTlz : name)) {
        setIsPunyfied(normalized);
      }

      // only check ownership if name is otherwise valid
      if (validities.length === 0 && normalized.length > 2) {
        try {
          const namehash = kinohash(normalized)

          const data = await client?.readContract({
            address: HYPERMAP,
            abi: hypermapAbi,
            functionName: "get",
            args: [namehash]
          })

          const tba = data?.[0];
          if (tba !== undefined) {
            setTba ? (setTba(tba)) : null;
          } else if (isReset) {
            validities.push(NAME_NOT_REGISTERED);
          }

          const owner = data?.[1];
          const owner_is_zero = owner === "0x0000000000000000000000000000000000000000";

          if (!owner_is_zero && !isReset) validities.push(NAME_CLAIMED);

          if (!owner_is_zero && isReset && address && owner !== address) validities.push(NAME_NOT_OWNER);

          if (isReset && owner_is_zero) validities.push(NAME_NOT_REGISTERED);
        } catch (e) {
          console.error({ e })
        }
      }
      setNameValidities(validities);
    }, 500);
  }, [name, triggerNameCheck, isReset]);

  const noSpaces = (e: any) =>
    e.target.value.indexOf(" ") === -1 && setName(e.target.value);

  return (
    <div className="enter-hns-name">
      <div className="input-wrapper">
        <input
          value={name}
          onChange={noSpaces}
          type="text"
          required
          name="hns-name"
          placeholder="node-name"
          className="hns-input"
        />
        {fixedTlz && <span className="hns-suffix">{fixedTlz}</span>}
      </div>
      {nameValidities.map((x, i) => (
        <p key={i} className="error-message">{x}</p>
      ))}
      {isPunyfied !== '' && <p className="puny-warning">special characters will be converted to punycode: {isPunyfied}</p>}
    </div>
  );
}

export default EnterHnsName;
