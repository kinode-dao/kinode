import { useNavigate, useLocation } from 'react-router-dom';

const steps = [
  { path: '/', label: 'Home' },
  { path: '/commit-os-name', label: 'Choose Name' },
  { path: '/mint-os-name', label: 'Mint Name' },
  { path: '/set-password', label: 'Set Password' },
];

interface ProgressBarProps {
  hnsName: string;
}

const ProgressBar = ({ hnsName }: ProgressBarProps) => {
  const navigate = useNavigate();
  const location = useLocation();
  
  const currentStepIndex = steps.findIndex(step => step.path === location.pathname);

  const isStepAccessible = (index: number) => {
    // Home is always accessible
    if (index === 0) return true;
    
    if (hnsName && index <= 2) return true;
    
    // Otherwise only allow going back
    return index <= currentStepIndex;
  };

  const handleStepClick = (path: string, index: number) => {
    if (isStepAccessible(index)) {
      navigate(path);
    }
  };

  return (
    <div className="progress-container">
      <div className="progress-bar">
        {steps.map((step, index) => {
          const accessible = isStepAccessible(index);
          return (
            <div key={step.path} className="step-wrapper">
              <div
                className={`step ${index <= currentStepIndex ? 'active' : ''} ${
                  index < currentStepIndex ? 'completed' : ''
                } ${accessible ? 'clickable' : 'disabled'}`}
                onClick={() => handleStepClick(step.path, index)}
              >
                <div className="step-number">{index}</div>
                <div className="step-label">{step.label}</div>
              </div>
              {index < steps.length - 1 && (
                <div className={`connector ${index < currentStepIndex ? 'active' : ''}`} />
              )}
            </div>
          );
        })}
      </div>
      {hnsName && (
        <div className="selected-name">
          Selected name: <span>{hnsName}</span>
        </div>
      )}
    </div>
  );
};

export default ProgressBar;
