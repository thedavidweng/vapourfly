using System.Text;

namespace Gameloop.Vdf;

internal sealed class CharBuffer
{
    readonly StringBuilder thiz;

    public CharBuffer() => thiz = new StringBuilder();

    public char this[int index]
    {
        get => thiz[index];
        set
        {
            var t = index - thiz.Length;
            for (int i = 0; i <= t; i++)
            {
                thiz.Append((char)default);
            }
            thiz[index] = value;
        }
    }

    public string ToString(int startIndex, int length) => thiz.ToString(startIndex, length);

    public override string ToString() => thiz.ToString();
}
