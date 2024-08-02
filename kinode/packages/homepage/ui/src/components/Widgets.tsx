import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import Widget from "./Widget"
import usePersistentStore from "../store/persistentStore"
import { DragDropContext, Draggable, DropResult, Droppable } from '@hello-pangea/dnd'
import { useEffect, useState } from "react"

const Widgets = () => {
  const { apps } = useHomepageStore()
  const { widgetSettings, widgetOrder, setWidgetOrder } = usePersistentStore();
  const [orderedWidgets, setOrderedWidgets] = useState<HomepageApp[]>([]);

  useEffect(() => {
    const visibleWidgets = apps.filter((app) => app.widget && !widgetSettings[app.id]?.hide);
    const orderedVisibleWidgets = visibleWidgets.sort((a, b) => {
      return widgetOrder.indexOf(a.id) - widgetOrder.indexOf(b.id);
    });
    setOrderedWidgets(orderedVisibleWidgets);
  }, [apps, widgetSettings, widgetOrder]);

  const onDragEnd = (result: DropResult) => {
    if (!result.destination) {
      return;
    }

    const reorderedWidgets = Array.from(orderedWidgets);
    const [reorderedItem] = reorderedWidgets.splice(result.source.index, 1);
    reorderedWidgets.splice(result.destination.index, 0, reorderedItem);

    const newWidgetOrder = reorderedWidgets.map(widget => widget.id);
    setWidgetOrder(newWidgetOrder);
    setOrderedWidgets(reorderedWidgets);
  }

  return (
    <DragDropContext onDragEnd={onDragEnd}>
      <Droppable droppableId="widgets">
        {(provided, _snapshot) => (
          <div
            id="widgets"
            ref={provided.innerRef}
            {...provided.droppableProps}
          >
            {orderedWidgets.map((app, index) => (
              <Draggable key={app.id} draggableId={app.id} index={index}>
                {(provided, _snapshot) => (
                  <div
                    ref={provided.innerRef}
                    {...provided.draggableProps}
                    {...provided.dragHandleProps}
                  >
                    <Widget
                      id={app.id}
                      label={app.label}
                      widget={app.widget!}
                    />
                  </div>
                )}
              </Draggable>
            ))}
            {provided.placeholder}
          </div>
        )}
      </Droppable>
    </DragDropContext>
  );
}

export default Widgets