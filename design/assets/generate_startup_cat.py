"""Assemble gpt-image-2.5 paw keyframes, preserving the fixed original scene.

Requires Pillow. Run with python3. Source: anywork-startup-cat-source.png,
created through imagegen CLI edit using the original anywork-app-icon.png and
the adjacent prompt file. Returned source has real alpha, which is preserved.
Only the two feathered paw regions change; no pixel warping is performed.
"""

from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
ASSETS = Path(__file__).resolve().parent
BRANDING = ROOT / 'code/anywork/assets/branding'
SIZE = 512


def paw_mask(points):
    mask = Image.new('L', (SIZE, SIZE))
    ImageDraw.Draw(mask).polygon(points, fill=255)
    return mask.filter(ImageFilter.GaussianBlur(1.5))


def main():
    source = Image.open(ASSETS / 'anywork-startup-cat-source.png').convert('RGBA')
    w,h = source.size
    poses = [source.crop((i%2*w//2,i//2*h//2,(i%2+1)*w//2,(i//2+1)*h//2))
             .resize((SIZE,SIZE),Image.Resampling.LANCZOS) for i in range(4)]
    base = poses[0]
    left = paw_mask([(145,313),(166,300),(223,300),(245,318),
                     (251,353),(243,379),(195,386),(147,371)])
    right = paw_mask([(251,345),(271,309),(345,308),(389,341),
                      (393,389),(366,420),(283,417),(250,399)])
    frames = [base, Image.composite(poses[1],base,left), base.copy(),
              Image.composite(poses[3],base,right)]
    # Keep some air around the cutout without changing the relative framing.
    padded = []
    for f in frames:
        canvas = Image.new('RGBA',(SIZE,SIZE))
        canvas.paste(f.resize((480,480),Image.Resampling.LANCZOS),(16,16))
        padded.append(canvas)
    frames = padded
    duration = [100,160,100,160]
    frames[0].save(BRANDING / 'anywork-startup-cat.png')
    frames[0].save(BRANDING / 'anywork-startup-cat.webp',save_all=True,
                   append_images=frames[1:],duration=duration,loop=0,lossless=True,method=6)
    palette = frames[0].convert('RGB').quantize(colors=255)
    indexed = []
    for f in frames:
        pal = f.convert('RGB').quantize(palette=palette,dither=Image.Dither.NONE)
        pal.paste(255,mask=f.getchannel('A').point(lambda a: 255 if a<128 else 0))
        indexed.append(pal)
    indexed[0].save(ASSETS / 'anywork-startup-cat.gif',save_all=True,
                    append_images=indexed[1:],duration=duration,loop=0,
                    transparency=255,disposal=2,optimize=False)
    sheet = Image.new('RGBA',(SIZE*2,SIZE*2))
    for i,f in enumerate(frames):
        sheet.paste(f,(i%2*SIZE,i//2*SIZE))
    sheet.save(ASSETS / 'anywork-startup-cat-frames.png')


if __name__ == '__main__':
    main()
