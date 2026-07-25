import React, { ReactNode } from 'react';

interface CardProps {
  children: ReactNode;
  className?: string;
  /** Optional card header title. Renders a hairline-separated header strip. */
  title?: ReactNode;
  /** Right-hand side of the header strip (actions, counts, filters). */
  actions?: ReactNode;
  /** Drop the body padding — for cards whose body is a full-bleed table. */
  flush?: boolean;
}

/**
 * The product's base surface: hairline border + 1px shadow, never a heavy
 * drop shadow. Cards carry structure, not decoration.
 */
export function Card({ children, className = '', title, actions, flush }: CardProps) {
  return (
    <div className={`card ${className}`}>
      {(title || actions) && (
        <div className="card-head">
          {typeof title === 'string' ? <h3 className="card-title">{title}</h3> : title}
          {actions && <div className="flex items-center gap-2">{actions}</div>}
        </div>
      )}
      <div className={flush ? '' : 'card-body'}>{children}</div>
    </div>
  );
}
