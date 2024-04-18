import React from "react";
import { FaCheck } from "react-icons/fa6";

export default function Checkbox({
  readOnly = false,
  checked,
  setChecked,
}: {
  readOnly?: boolean;
  checked: boolean;
  setChecked?: (checked: boolean) => void;
}) {
  return (
    <div className="relative">
      <input
        type="checkbox"
        id="checked"
        name="checked"
        checked={checked}
        onChange={(e) => setChecked && setChecked(e.target.checked)}
        autoFocus
        readOnly={readOnly}
      />
      {checked && (
        <FaCheck
          className="absolute left-1 top-1 cursor-pointer"
          onClick={() => setChecked && setChecked(false)}
        />
      )}
    </div>
  );
}
