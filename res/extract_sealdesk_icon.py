from pathlib import Path
from PIL import Image,ImageDraw,ImageFilter
from statistics import median
b=Path(__file__).resolve().parents[1]
src=Image.open(b/'res/sealdesk-icon-approved-sheet.png').convert('RGB')

# Teal outer boundary distinguishes the artwork from the neutral presentation background.
rows={}
for y in range(55,765):
 xs=[]
 for x in range(130,880):
  r,g,bl=src.getpixel((x,y))
  if g-r>24 and bl-r>24 and r<145:xs.append(x)
 if xs:rows[y]=(min(xs),max(xs))
x0=min(v[0] for v in rows.values());x1=max(v[1] for v in rows.values())+1;y0=min(rows);y1=max(rows)+1
im=src.crop((x0,y0,x1,y1)).convert('RGBA');mask=Image.new('L',im.size);d=ImageDraw.Draw(mask)
for y,(a,z) in rows.items():
 neighbors=[rows[j] for j in range(y-5,y+6) if j in rows]
 a,z=round(median(v[0] for v in neighbors)),round(median(v[1] for v in neighbors))
 d.line((a-x0,y-y0,z-x0,y-y0),fill=255)
mask=mask.filter(ImageFilter.MinFilter(3)).filter(ImageFilter.GaussianBlur(.65));im.putalpha(mask)
n=max(im.size)+32;master=Image.new('RGBA',(n,n));master.alpha_composite(im,((n-im.width)//2,(n-im.height)//2));master.save(b/'res/sealdesk-icon-approved.png')
# Monochrome platform templates are sampled from the same original artwork, not redrawn.
mono=Image.new('RGBA',master.size);pix=master.load();out=mono.load()
for y in range(n):
 for x in range(n):
  r,g,bl,a=pix[x,y]
  lum=(r+g+bl)/3
  # Seal ivory and the mint cat are substantially brighter than the ocean tile.
  coverage=max(0,min(1,(lum-145)/25))*a
  if y<n*.15 or x<n*.1 or x>n*.9 or a<250:coverage=0
  out[x,y]=(255,255,255,round(coverage))
mono.save(b/'res/sealdesk-icon-monochrome.png')
print('Original crop:',(x0,y0,x1,y1),'artwork',im.size,'master',master.size)
