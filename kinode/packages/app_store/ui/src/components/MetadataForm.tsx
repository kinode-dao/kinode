import React, { useCallback, useEffect, useState } from "react";
import { AppInfo } from "../types/Apps";
import { FaX } from "react-icons/fa6";

interface Props {
  app?: AppInfo;
  packageName: string;
  publisherId: string;
  goBack: () => void;
}

const VALID_VERSION_REGEX = /^\d+\.\d+\.\d+$/;

const MetadataForm = ({ app, packageName, publisherId, goBack }: Props) => {
  const [formData, setFormData] = useState({
    name: app?.metadata?.name || "",
    description: app?.metadata?.description || "",
    image: app?.metadata?.image || "",
    external_url: app?.metadata?.external_url || "",
    animation_url: app?.metadata?.animation_url || "",
    // properties, which can come from the app itself
    package_name: packageName,
    current_version: "",
    publisher: publisherId,
    mirrors: [publisherId],
  });

  const [codeHashes, setCodeHashes] = useState<[string, string][]>(
    Object.entries(app?.metadata?.properties?.code_hashes || {}).concat([
      ["", app?.state?.our_version || ""],
    ])
  );

  const handleFieldChange = (field, value) => {
    setFormData({
      ...formData,
      [field]: value,
    });
  };

  useEffect(() => {
    handleFieldChange("package_name", packageName);
  }, [packageName]);

  useEffect(() => {
    handleFieldChange("publisher", publisherId);
  }, [publisherId]);

  const handleSubmit = useCallback(() => {
    const code_hashes = codeHashes.reduce((acc, [version, hash]) => {
      acc[version] = hash;
      return acc;
    }, {});

    if (!VALID_VERSION_REGEX.test(formData.current_version)) {
      window.alert("Current version must be in the format x.y.z");
      return;
    } else if (!code_hashes[formData.current_version]) {
      window.alert(
        `Code hashes must include current version (${formData.current_version})`
      );
      return;
    } else if (
      !Object.keys(code_hashes).reduce(
        (valid, version) => valid && VALID_VERSION_REGEX.test(version),
        true
      )
    ) {
      window.alert("Code hashes must be a JSON object with valid version keys");
      return;
    }

    const jsonData = JSON.stringify({
      name: formData.name,
      description: formData.description,
      image: formData.image,
      external_url: formData.external_url,
      animation_url: formData.animation_url,
      properties: {
        package_name: formData.package_name,
        current_version: formData.current_version,
        publisher: formData.publisher,
        mirrors: formData.mirrors,
        code_hashes,
      },
    });

    const blob = new Blob([jsonData], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download =
      formData.package_name + "_" + formData.publisher + "_metadata.json";
    a.click();
    URL.revokeObjectURL(url);
  }, [formData, codeHashes]);

  const handleClearForm = () => {
    setFormData({
      name: "",
      description: "",
      image: "",
      external_url: "",
      animation_url: "",

      package_name: "",
      current_version: "",
      publisher: "",
      mirrors: [],
    });
    setCodeHashes([]);
  };

  return (
    <form className="flex flex-col card mt-2 gap-2">
      <h4>Fill out metadata</h4>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Name</label>
        <input
          type="text"
          placeholder="Name"
          value={formData.name}
          onChange={(e) => handleFieldChange("name", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Description</label>
        <input
          type="text"
          placeholder="Description"
          value={formData.description}
          onChange={(e) => handleFieldChange("description", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Image URL</label>
        <input
          type="text"
          placeholder="Image URL"
          value={formData.image}
          onChange={(e) => handleFieldChange("image", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">External URL</label>
        <input
          type="text"
          placeholder="External URL"
          value={formData.external_url}
          onChange={(e) => handleFieldChange("external_url", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Animation URL</label>
        <input
          type="text"
          placeholder="Animation URL"
          value={formData.animation_url}
          onChange={(e) => handleFieldChange("animation_url", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Package Name</label>
        <input
          type="text"
          placeholder="Package Name"
          value={formData.package_name}
          onChange={(e) => handleFieldChange("package_name", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Current Version</label>
        <input
          type="text"
          placeholder="Current Version"
          value={formData.current_version}
          onChange={(e) => handleFieldChange("current_version", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Publisher</label>
        <input
          type="text"
          placeholder="Publisher"
          value={formData.publisher}
          onChange={(e) => handleFieldChange("publisher", e.target.value)}
        />
      </div>
      <div className="flex flex-col w-3/4">
        <label className="metadata-label">Mirrors (separated by commas)</label>
        <input
          type="text"
          placeholder="Mirrors (separated by commas)"
          value={formData.mirrors.join(",")}
          onChange={(e) =>
            handleFieldChange(
              "mirrors",
              e.target.value.split(",").map((m) => m.trim())
            )
          }
        />
      </div>
      <div
        className="flex flex-col w-3/4 gap-2"
      >
        <div
          className="flex gap-2 mt-0 justify-between w-full"
        >
          <h5 className="m-0">Code Hashes</h5>
          <button
            type="button"
            onClick={() => setCodeHashes([...codeHashes, ["", ""]])}
            className="clear"
          >
            Add code hash
          </button>
        </div>

        {codeHashes.map(([version, hash], ind, arr) => (
          <div
            key={ind + "_code_hash"}
            className="flex gap-2 mt-0 w-full"
          >
            <input
              type="text"
              placeholder="Version"
              value={version}
              onChange={(e) =>
                setCodeHashes((prev) => {
                  const newHashes = [...prev];
                  newHashes[ind][0] = e.target.value;
                  return newHashes;
                })
              }
              className="flex-1"
            />
            <input
              type="text"
              placeholder="Hash"
              value={hash}
              onChange={(e) =>
                setCodeHashes((prev) => {
                  const newHashes = [...prev];
                  newHashes[ind][1] = e.target.value;
                  return newHashes;
                })
              }
              className="flex-5"
            />
            {arr.length > 1 && (
              <button
                type="button"
                onClick={() =>
                  setCodeHashes((prev) => prev.filter((_, i) => i !== ind))
                }
                className="icon"
              >
                <FaX />
              </button>
            )}
          </div>
        ))}
      </div>
      <div className="flex gap-2 my-4">
        <button type="button" onClick={handleSubmit} className="alt">
          Download JSON
        </button>
        <button type="button" onClick={handleClearForm} className="clear">
          Clear Form
        </button>
        <button type="button" onClick={goBack}>
          Done
        </button>
      </div>
    </form>
  );
};

export default MetadataForm;
