export interface SpringAxis {
  position: number;
  velocity: number;
}

// Closed-form critically damped spring. Velocity carries through changes of target.
export function advanceSpring(axis: SpringAxis, target: number, dt: number) {
  const frequency = 11;
  const displacement = axis.position - target;
  const impulse = axis.velocity + frequency * displacement;
  const decay = Math.exp(-frequency * dt);
  axis.position = target + (displacement + impulse * dt) * decay;
  axis.velocity = (axis.velocity - frequency * impulse * dt) * decay;
}
