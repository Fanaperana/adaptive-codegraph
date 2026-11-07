import { Listener } from "./events";

export class Logger { // @def Logger
  log(msg: string): void { // @def Logger::log
    console.log(this.format(msg));
  }

  protected format(msg: string): string { // @def Logger::format
    return `[log] ${msg}`;
  }
}

export class AuditLogger extends Logger implements Listener { // @def AuditLogger
  handle(event: string): void { // @def AuditLogger::handle
    this.log(event);
  }
}

export default function createLogger(): Logger { // @def createLogger
  return new Logger();
}
