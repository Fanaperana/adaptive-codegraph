import createLogger, { AuditLogger } from "./logger";
import { EventBus } from "./events";

export class App { // @def App
  private bus: EventBus = new EventBus(); // @def App::bus
  private logger = createLogger(); // @def App::logger

  start(): void { // @def App::start
    this.bus.subscribe(new AuditLogger());
    this.bus.publish("start");
    this.logger.log("started");
  }
}

export function boot(): App { // @def boot
  const app = new App();
  app.start();
  return app;
}
