import React, { useEffect, useRef } from "react";
import { hooks } from "../connectors/metamask";
import { DotOsRegistrar } from "../abis/types";
import isValidDomain from 'is-valid-domain'
import { hash } from 'eth-ens-namehash'
import { toAscii } from 'idna-uts46-hx'

global.Buffer = global.Buffer || require('buffer').Buffer;

const {
  useChainId,
  useProvider,
} = hooks;

type ClaimOsNameProps = {
  name: string,
  setName: React.Dispatch<React.SetStateAction<string>>
  nameValidities: string[],
  setNameValidities: React.Dispatch<React.SetStateAction<string[]>>,
  dotOs: DotOsRegistrar,
  triggerNameCheck: boolean
}

function EnterOsName({
  name,
  setName,
  nameValidities,
  setNameValidities,
  dotOs,
  triggerNameCheck
}: ClaimOsNameProps) {

  const NAME_URL = "Name must be a valid URL without subdomains (A-Z, a-z, 0-9, and punycode)"
  const NAME_LENGTH = "Name must be 9 characters or more"
  const NAME_CLAIMED = "Name is already claimed"
  const NAME_INVALID_PUNY = "Unsupported punycode character"

  const debouncer = useRef<NodeJS.Timeout | null>(null)

  useEffect(() => {

    if (debouncer.current)
      clearTimeout(debouncer.current);

    debouncer.current = setTimeout(async () => {

      let index: number
      let validities = [...nameValidities]

      const len = [...name].length
      index = validities.indexOf(NAME_LENGTH)
      if (len < 9 && len != 0) {
        if (index == -1) validities.push(NAME_LENGTH)
      } else if (index != -1) validities.splice(index, 1)

      let normalized: string
      index = validities.indexOf(NAME_INVALID_PUNY)
      try {
        normalized = toAscii(name + ".os")
        if (index != -1) validities.splice(index, 1)
      } catch (e) {
        if (index == -1) validities.push(NAME_INVALID_PUNY)
      }

      // only check if name is valid punycode
      if (normalized! !== undefined) {

        index = validities.indexOf(NAME_URL)
        if (name != "" && !isValidDomain(normalized)) {
          if (index == -1) validities.push(NAME_URL)
        } else if (index != -1) validities.splice(index, 1)

        index = validities.indexOf(NAME_CLAIMED)
        if (validities.length == 0 || index != -1) {
          try {
            await dotOs.ownerOf(hash(normalized))
            if (index == -1) validities.push(NAME_CLAIMED)
          } catch (e) {
            if (index != -1) validities.splice(index, 1)
          }
        }
      }

      setNameValidities(validities)

    }, 500)
  }, [name, triggerNameCheck])

  const noDots = (e: any) => e.target.value.indexOf('.') == -1
    && setName(e.target.value)

  return (
    <div className="col" style={{ width: '100%' }}>
      <div className="row" style={{ width: '100%' }}>
        <input
          value={name}
          onChange={noDots}
          type="text"
          required
          name="dot-os-name"
          placeholder="e.g. myname"
        />
        <div className="os">.os</div>
      </div>
      {nameValidities.map((x, i) => <div key={i}><br /><span className="name-validity">{x}</span></div>)}
    </div>
  )

}

export default EnterOsName;
