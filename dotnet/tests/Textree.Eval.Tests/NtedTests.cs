using Textree.Eval;
using Xunit;
using static Textree.Eval.Nted.Node;

namespace Textree.Eval.Tests;

public class NtedTests
{
    private static Nted.Node Tree(params (string Doc, string Folder)[] docs) =>
        Nted.TreeOf(docs.Select(d => KeyValuePair.Create(d.Doc, d.Folder)));

    [Fact]
    public void The_same_tree_is_at_distance_zero_whatever_the_folder_names()
    {
        var a = Tree(("x", "garden"), ("y", "garden"), ("z", "books"));
        var b = Tree(("x", "plants"), ("y", "plants"), ("z", "reading"));
        Assert.Equal(0, Nted.Distance(a, b));
    }

    [Fact]
    public void Moving_one_document_to_the_other_folder_costs_two()
    {
        var a = Tree(("x", "garden"), ("y", "garden"), ("z", "books"));
        var b = Tree(("x", "garden"), ("y", "books"), ("z", "books"));
        // Delete leaf y under one folder, insert it under the other.
        Assert.Equal(2, Nted.Distance(a, b));
        Assert.Equal(2.0 / (a.Size + b.Size), Nted.Normalized(a, b));
    }

    [Fact]
    public void A_flat_tree_against_a_grouped_one_keeps_one_group_whole()
    {
        var grouped = Tree(("x", "garden"), ("y", "garden"), ("z", "books"));
        var flat = Tree(("x", ""), ("y", ""), ("z", ""));
        // The constrained distance keeps disjoint subtrees apart: removing both folders (2) would put
        // garden's and books' documents under one parent, so one group is moved instead (delete the
        // two folders' nodes 2, plus z out and in 2) — 4, above the unconstrained 2.
        Assert.Equal(4, Nted.Distance(grouped, flat));
    }

    [Fact]
    public void Trees_sharing_nothing_cost_every_node_but_the_roots()
    {
        var a = Folder(Leaf("x"), Leaf("y"));
        var b = Folder(Leaf("p"), Leaf("q"));
        Assert.Equal(2, Nted.Distance(a, b)); // two relabels
        Assert.Equal(0, Nted.Distance(a, a));
    }

    [Fact]
    public void The_assignment_finds_the_cheapest_matching()
    {
        Assert.Equal(5, Hungarian.Solve(new[,] { { 4, 1, 3 }, { 2, 0, 5 }, { 3, 2, 2 } }));
    }
}
