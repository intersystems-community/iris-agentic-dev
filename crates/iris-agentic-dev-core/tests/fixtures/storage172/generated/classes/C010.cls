Class IadSm172Gen.C010 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property K3 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On (K1, K2, K3) [ IdKey, Unique ];

Index IdxF1 On F1;

Storage SQLStorage
{
<SQLMap name="IdxF1">
<Global>^IadSm172Gen010I</Global>
<Subscript name="1">
<Expression>{F1}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{K2}</Expression>
</Subscript>
<Subscript name="4">
<Expression>{K3}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<SQLMap name="Master">
<Data name="F1">
<Delimiter>"|"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>"|"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Gen010</Global>
<Subscript name="1">
<AccessType>Sub</AccessType>
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<AccessType>Sub</AccessType>
<Expression>{K2}</Expression>
</Subscript>
<Subscript name="3">
<AccessType>Sub</AccessType>
<Expression>{K3}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen010S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
