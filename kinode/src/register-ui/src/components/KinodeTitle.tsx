import kinodeLogo from "../assets/kinode.svg";

export const KinodeTitle: React.FC<{ prefix: string, showLogo?: boolean }> = ({ prefix, showLogo }) => {

  return (
    <div className="mb-4 flex flex-col c">
      <h1>{prefix}</h1>
      {showLogo && <>
        <h1 className="display text-5xl mt-10 mb-8 ml-4">Kinode<span className="text-xs">&reg;</span></h1>
        <img src={kinodeLogo} className="w-32 h-32" />
      </>}
    </div>
  );
};
