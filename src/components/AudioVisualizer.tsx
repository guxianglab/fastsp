import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

interface AudioVisualizerProps {
    isRecording: boolean;
    className?: string;
}

class Particle {
    x: number;
    y: number;
    radius: number;
    color: string;
    velocity: { x: number; y: number };
    alpha: number;
    baseRadius: number;
    angle: number;
    distance: number;
    baseDistance: number;

    constructor(canvasWidth: number, canvasHeight: number) {
        this.angle = Math.random() * Math.PI * 2;
        this.baseDistance = Math.random() * 30 + 20;
        this.distance = this.baseDistance;
        this.x = canvasWidth / 2 + Math.cos(this.angle) * this.distance;
        this.y = canvasHeight / 2 + Math.sin(this.angle) * this.distance;
        this.baseRadius = Math.random() * 15 + 8;
        this.radius = this.baseRadius;
        this.color = `hsl(${220 + Math.random() * 40}, 70%, 50%)`;
        this.velocity = {
            x: (Math.random() - 0.5) * 0.5,
            y: (Math.random() - 0.5) * 0.5
        };
        this.alpha = Math.random() * 0.4 + 0.2;
    }

    update(level: number, width: number, height: number) {
        const centerX = width / 2;
        const centerY = height / 2;

        // Update angle for rotation
        this.angle += 0.005 + level * 0.02;

        // Distance expands with audio level
        const targetDistance = this.baseDistance + level * 80;
        this.distance += (targetDistance - this.distance) * 0.15;

        // Position based on angle and distance
        this.x = centerX + Math.cos(this.angle) * this.distance + this.velocity.x;
        this.y = centerY + Math.sin(this.angle) * this.distance + this.velocity.y;

        // Radius pulses with level
        this.radius = this.baseRadius * (1 + level * 1.5);

        // Alpha increases with level
        this.alpha = 0.2 + level * 0.5;
    }

    draw(ctx: CanvasRenderingContext2D) {
        ctx.beginPath();
        ctx.arc(this.x, this.y, this.radius, 0, Math.PI * 2);
        ctx.fillStyle = this.color;
        ctx.globalAlpha = this.alpha;
        ctx.fill();
        ctx.globalAlpha = 1.0;
    }
}

// Ring wave that expands outward
class RingWave {
    radius: number;
    maxRadius: number;
    alpha: number;
    lineWidth: number;

    constructor(initialRadius: number, maxRadius: number) {
        this.radius = initialRadius;
        this.maxRadius = maxRadius;
        this.alpha = 0.6;
        this.lineWidth = 3;
    }

    update(): boolean {
        this.radius += 3;
        this.alpha -= 0.015;
        this.lineWidth = Math.max(1, this.lineWidth - 0.05);
        return this.alpha > 0 && this.radius < this.maxRadius;
    }

    draw(ctx: CanvasRenderingContext2D, centerX: number, centerY: number) {
        ctx.beginPath();
        ctx.arc(centerX, centerY, this.radius, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(99, 102, 241, ${this.alpha})`;
        ctx.lineWidth = this.lineWidth;
        ctx.stroke();
    }
}

export function AudioVisualizer({ isRecording, className }: AudioVisualizerProps) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const audioLevelRef = useRef(0);
    const smoothedLevelRef = useRef(0);
    const animationFrameRef = useRef<number | null>(null);
    const particlesRef = useRef<Particle[]>([]);
    const ringsRef = useRef<RingWave[]>([]);
    const lastRingTimeRef = useRef(0);

    useEffect(() => {
        // Initialize particles
        if (particlesRef.current.length === 0) {
            for (let i = 0; i < 24; i++) {
                particlesRef.current.push(new Particle(300, 300));
            }
        }

        const unlisten = listen<number>("audio_level", (event) => {
            // Boost and clamp the level
            const raw = Math.min(event.payload * 6, 1.0);
            // Apply non-linear curve to amplify small values for visual effect
            // sqrt makes small values more visible without over-amplifying loud sounds
            audioLevelRef.current = Math.sqrt(raw);
        });

        return () => {
            unlisten.then(f => f());
        };
    }, []);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        const render = () => {
            const now = Date.now();

            // Smooth the audio level
            if (isRecording) {
                smoothedLevelRef.current += (audioLevelRef.current - smoothedLevelRef.current) * 0.25;
            } else {
                smoothedLevelRef.current *= 0.9;
                audioLevelRef.current *= 0.9;
            }

            const level = smoothedLevelRef.current;

            ctx.clearRect(0, 0, canvas.width, canvas.height);

            const centerX = canvas.width / 2;
            const centerY = canvas.height / 2;

            // Create ring waves when level spikes
            if (isRecording && level > 0.3 && now - lastRingTimeRef.current > 200) {
                ringsRef.current.push(new RingWave(40, 120));
                lastRingTimeRef.current = now;
            }

            // Draw and update rings
            ctx.globalCompositeOperation = "source-over";
            ringsRef.current = ringsRef.current.filter(ring => {
                const alive = ring.update();
                if (alive) ring.draw(ctx, centerX, centerY);
                return alive;
            });

            // Composite mode for glow effect
            ctx.globalCompositeOperation = "screen";

            // Draw outer glow that scales with level
            const outerRadius = 50 + level * 70;
            const outerGradient = ctx.createRadialGradient(centerX, centerY, 0, centerX, centerY, outerRadius);
            outerGradient.addColorStop(0, `rgba(99, 102, 241, ${0.1 + level * 0.2})`);
            outerGradient.addColorStop(0.6, `rgba(99, 102, 241, ${0.05 + level * 0.1})`);
            outerGradient.addColorStop(1, "rgba(99, 102, 241, 0)");

            ctx.fillStyle = outerGradient;
            ctx.beginPath();
            ctx.arc(centerX, centerY, outerRadius, 0, Math.PI * 2);
            ctx.fill();

            // Draw particles
            particlesRef.current.forEach(p => {
                p.update(level, canvas.width, canvas.height);
                p.draw(ctx);
            });

            // Draw core energy
            const coreRadius = 25 + level * 35;
            const gradient = ctx.createRadialGradient(centerX, centerY, 0, centerX, centerY, coreRadius);
            gradient.addColorStop(0, `rgba(255, 255, 255, ${0.7 + level * 0.3})`);
            gradient.addColorStop(0.4, `rgba(129, 140, 248, ${0.5 + level * 0.3})`);
            gradient.addColorStop(1, "rgba(99, 102, 241, 0)");

            ctx.fillStyle = gradient;
            ctx.beginPath();
            ctx.arc(centerX, centerY, coreRadius, 0, Math.PI * 2);
            ctx.fill();

            ctx.globalCompositeOperation = "source-over";

            animationFrameRef.current = requestAnimationFrame(render);
        };

        render();

        return () => {
            if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
        };
    }, [isRecording]);

    return (
        <canvas
            ref={canvasRef}
            width={300}
            height={300}
            className={`pointer-events-none rounded-full ${className || "w-32 h-32"}`}
        />
    );
}
