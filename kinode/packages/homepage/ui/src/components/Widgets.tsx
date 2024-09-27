import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import Widget from "./Widget"
import usePersistentStore from "../store/persistentStore"
import { useEffect, useState } from "react"

const Widgets = () => {
  const { apps } = useHomepageStore()
  const { widgetSettings, widgetOrder, setWidgetOrder } = usePersistentStore();
  const [orderedWidgets, setOrderedWidgets] = useState<HomepageApp[]>([]);

  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);

  useEffect(() => {
    const visibleWidgets = apps.filter((app) => app.widget && !widgetSettings[app.id]?.hide);
    const orderedVisibleWidgets = visibleWidgets.sort((a, b) => {
      return widgetOrder.indexOf(a.id) - widgetOrder.indexOf(b.id);
    });
    setOrderedWidgets(orderedVisibleWidgets);
  }, [apps, widgetSettings, widgetOrder]);

  const handleDragStart = (e: React.DragEvent, index: number) => {
    e.dataTransfer.setData("text/plain", index.toString());
    setDraggedIndex(index);
  };

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    setDragOverIndex(index);
  };

  const handleDragEnd = () => {
    setDraggedIndex(null);
    setDragOverIndex(null);
  };

  const handleDrop = (e: React.DragEvent, dropIndex: number) => {
    e.preventDefault();
    const dragIndex = parseInt(e.dataTransfer.getData("text/plain"), 10);
    if (dragIndex === dropIndex) return;

    const newSortedWidgets = [...orderedWidgets];
    const [movedWidget] = newSortedWidgets.splice(dragIndex, 1);
    newSortedWidgets.splice(dropIndex, 0, movedWidget);

    const newWidgetOrder = newSortedWidgets.map((wid) => wid.id);
    setWidgetOrder(newWidgetOrder);
    handleDragEnd();
  };

  return (
    <div
      id="widgets"
    >
      {orderedWidgets.map((wid, index) => (
        <div
          key={wid.id}
          draggable
          onDragStart={(e) => handleDragStart(e, index)}
          onDragOver={(e) => handleDragOver(e, index)}
          onDragEnd={handleDragEnd}
          onDrop={(e) => handleDrop(e, index)}
          className={`widget-wrapper ${draggedIndex === index ? "dragging" : ""
            } ${dragOverIndex === index ? "drag-over" : ""}`}
        >
          <Widget
            id={wid.id}
            label={wid.label}
            widget={wid.widget!}
          />
          <div className="drag-handle">⋮⋮</div>
        </div>
      ))}
    </div>
  );
}

export default Widgets